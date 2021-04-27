use bytes::Bytes;
use chain::{Block, IndexedBlock, IndexedBlockHeader};
use hash::H256;
use kv::{
    AutoFlushingOverlayDatabase, CacheDatabase, DatabaseConfig, DiskDatabase, Key, KeyState,
    KeyValue, KeyValueDatabase, MemoryDatabase, OverlayDatabase, Transaction as DBTransaction,
    Value,
};
use kv::{COL_BLOCKS, COL_BLOCK_HASHES, COL_BLOCK_NUMBERS, COL_COUNT};
use parking_lot::RwLock;
use ser::{deserialize, serialize};
use std::fs;
use std::path::Path;
use storage::{
    BestBlock, BlockChain, BlockHeaderProvider, BlockOrigin, BlockProvider, BlockRef, CanonStore,
    ConfigStore, Error, ForkChain, Forkable, SideChainOrigin, Store,
};

const KEY_BEST_BLOCK_NUMBER: &'static str = "best_block_number";
const KEY_BEST_BLOCK_HASH: &'static str = "best_block_hash";

const MAX_FORK_ROUTE_PRESET: usize = 2048;

pub struct BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    best_block: RwLock<BestBlock>,
    db: T,
}

pub struct ForkChainDatabase<'a, T>
where
    T: 'a + KeyValueDatabase,
{
    blockchain: BlockChainDatabase<OverlayDatabase<'a, T>>,
}

impl<'a, T> ForkChain for ForkChainDatabase<'a, T>
where
    T: KeyValueDatabase,
{
    fn store(&self) -> &dyn Store {
        &self.blockchain
    }

    fn flush(&self) -> Result<(), Error> {
        self.blockchain.db.flush().map_err(Error::DatabaseError)
    }
}

impl BlockChainDatabase<CacheDatabase<AutoFlushingOverlayDatabase<DiskDatabase>>> {
    pub fn open_at_path<P>(path: P, total_cache: usize) -> Result<Self, Error>
    where
        P: AsRef<Path>,
    {
        fs::create_dir_all(path.as_ref()).map_err(|err| Error::DatabaseError(err.to_string()))?;
        let mut cfg = DatabaseConfig::with_columns(Some(COL_COUNT));

        // TODO:
        // cfg.set_cache(Some(COL_TRANSACTIONS), total_cache / 4);
        // cfg.set_cache(Some(COL_TRANSACTIONS_META), total_cache / 4);
        cfg.set_cache(Some(COL_BLOCKS), total_cache / 4);

        cfg.set_cache(Some(COL_BLOCK_HASHES), total_cache / 12);
        // TODO:
        // cfg.set_cache(Some(COL_BLOCK_TRANSACTIONS), total_cache / 12);
        cfg.set_cache(Some(COL_BLOCK_NUMBERS), total_cache / 12);

        match DiskDatabase::open(cfg, path) {
            Ok(db) => Ok(Self::open_with_cache(db)),
            Err(err) => Err(Error::DatabaseError(err)),
        }
    }
}

impl BlockChainDatabase<MemoryDatabase> {
    pub fn init_test_chain(blocks: Vec<IndexedBlock>) -> Self {
        let store = BlockChainDatabase::open(MemoryDatabase::default());

        for block in blocks {
            let hash = block.hash().clone();
            store.insert(block).unwrap();
            store.canonize(&hash).unwrap();
        }
        store
    }
}

impl<T> BlockChainDatabase<CacheDatabase<AutoFlushingOverlayDatabase<T>>>
where
    T: KeyValueDatabase,
{
    pub fn open_with_cache(db: T) -> Self {
        let db = CacheDatabase::new(AutoFlushingOverlayDatabase::new(db, 50));
        let best_block = Self::read_best_block(&db).unwrap_or_default();
        BlockChainDatabase {
            best_block: RwLock::new(best_block),
            db: db,
        }
    }
}

impl<T> BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    fn read_best_block(db: &T) -> Option<BestBlock> {
        let best_number = db
            .get(&Key::Meta(KEY_BEST_BLOCK_NUMBER))
            .map(KeyState::into_option)
            .map(|x| x.and_then(Value::as_meta));
        let best_hash = db
            .get(&Key::Meta(KEY_BEST_BLOCK_HASH))
            .map(KeyState::into_option)
            .map(|x| x.and_then(Value::as_meta));

        match (best_number, best_hash) {
            (Ok(None), Ok(None)) => None,
            (Ok(Some(number)), Ok(Some(hash))) => Some(BestBlock {
                number: deserialize(&**number)
                    .expect("Inconsistent DB. Invalid best block number."),
                hash: deserialize(&**hash).expect("Inconsistent DB. Invalid best block hash."),
            }),
            _ => panic!("Inconsistent DB"),
        }
    }

    pub fn open(db: T) -> Self {
        let best_block = Self::read_best_block(&db).unwrap_or_default();
        BlockChainDatabase {
            best_block: RwLock::new(best_block),
            db: db,
        }
    }

    pub fn best_block(&self) -> BestBlock {
        self.best_block.read().clone()
    }

    pub fn fork(&self, side_chain: SideChainOrigin) -> Result<ForkChainDatabase<T>, Error> {
        let overlay = BlockChainDatabase::open(OverlayDatabase::new(&self.db));

        for hash in side_chain.decanonized_route.into_iter().rev() {
            let decanonized_hash = overlay.decanonize()?;
            assert_eq!(hash, decanonized_hash);
        }

        for block_hash in &side_chain.canonized_route {
            overlay.canonize(block_hash)?;
        }

        let fork = ForkChainDatabase {
            blockchain: overlay,
        };

        Ok(fork)
    }

    pub fn switch_to_fork(&self, fork: ForkChainDatabase<T>) -> Result<(), Error> {
        let mut best_block = self.best_block.write();
        *best_block = fork.blockchain.best_block.read().clone();
        fork.blockchain.db.flush().map_err(Error::DatabaseError)
    }

    pub fn block_origin(&self, header: &IndexedBlockHeader) -> Result<BlockOrigin, Error> {
        let best_block = self.best_block.read();
        assert_eq!(
            Some(best_block.hash.clone()),
            self.block_hash(best_block.number)
        );
        if self.contains_block(header.hash.clone().into()) {
            // it does not matter if it's canon chain or side chain block
            return Ok(BlockOrigin::KnownBlock);
        }

        if best_block.hash == header.raw.previous_header_hash {
            return Ok(BlockOrigin::CanonChain {
                block_number: best_block.number + 1,
            });
        }

        if !self.contains_block(header.raw.previous_header_hash.clone().into()) {
            return Err(Error::UnknownParent);
        }

        let mut sidechain_route = Vec::new();
        let mut next_hash = header.raw.previous_header_hash.clone();

        for fork_len in 0..MAX_FORK_ROUTE_PRESET {
            match self.block_number(&next_hash) {
                Some(number) => {
                    let block_number = number + fork_len as u32 + 1;
                    let origin = SideChainOrigin {
                        ancestor: number,
                        canonized_route: sidechain_route.into_iter().rev().collect(),
                        decanonized_route: (number + 1..best_block.number + 1)
                            .into_iter()
                            .filter_map(|decanonized_bn| self.block_hash(decanonized_bn))
                            .collect(),
                        block_number: block_number,
                    };
                    if block_number > best_block.number {
                        return Ok(BlockOrigin::SideChainBecomingCanonChain(origin));
                    } else {
                        return Ok(BlockOrigin::SideChain(origin));
                    }
                }
                None => {
                    sidechain_route.push(next_hash.clone());
                    next_hash = self
                        .block_header(next_hash.into())
                        .expect("not to find orphaned side chain in database; qed")
                        .raw
                        .previous_header_hash;
                }
            }
        }

        Err(Error::AncientFork)
    }

    pub fn insert(&self, block: IndexedBlock) -> Result<(), Error> {
        if self.contains_block(block.hash().clone().into()) {
            return Ok(());
        }

        let parent_hash = block.header.raw.previous_header_hash.clone();
        if !self.contains_block(parent_hash.clone().into()) && !parent_hash.is_zero() {
            return Err(Error::UnknownParent);
        }

        let mut update = DBTransaction::new();
        update.insert(KeyValue::Block(
            block.hash().clone(),
            Block {
                block_header: block.header.raw,
                proof: block.proof,
            },
        ));

        self.db.write(update).map_err(Error::DatabaseError)
    }

    /// Rollbacks single best block
    // TODO:
    // 1. implement this
    // 2. consider update randomness data or metadata
    fn rollback_best(&self) -> Result<H256, Error> {
        unimplemented!()

        // let best_block_hash = self.best_block.read().hash.clone();
        // let tx_to_decanonize = self.block_transaction_hashes(best_block_hash.into());
        // let decanonized_hash = self.decanonize()?;
        // debug_assert_eq!(best_block_hash, decanonized_hash);

        // // and now remove decanonized block from database
        // // all code currently works in assumption that origin of all blocks is one of:
        // // {CanonChain, SideChain, SideChainBecomingCanonChain}
        // let mut update = DBTransaction::new();
        // update.delete(Key::BlockHeader(decanonized_hash.clone()));
        // update.delete(Key::BlockTransactions(decanonized_hash.clone()));
        // for tx_hash in tx_to_decanonize {
        //     update.delete(Key::Transaction(tx_hash));
        // }

        // self.db.write(update).map_err(Error::DatabaseError)?;

        // Ok(self.best_block().hash)
    }

    /// Marks block as a new best block.
    /// Block must be already inserted into db, and it's parent must be current best block.
    /// Updates meta data.
    pub fn canonize(&self, hash: &H256) -> Result<(), Error> {
        let mut best_block = self.best_block.write();
        let block = match self.block(hash.clone().into()) {
            Some(block) => block,
            None => {
                error!(target: "db", "Block is not found during canonization: {}", hash.reversed());
                return Err(Error::CannotCanonize);
            }
        };

        if best_block.hash != block.header.raw.previous_header_hash {
            error!(
                target: "db",
                "Wrong best block during canonization. Best {}, parent: {}",
                best_block.hash.reversed(),
                block.header.raw.previous_header_hash.reversed(),
            );
            return Err(Error::CannotCanonize);
        }

        let new_best_block = BestBlock {
            hash: hash.clone(),
            number: if block.header.raw.previous_header_hash.is_zero() {
                assert_eq!(best_block.number, 0);
                0
            } else {
                best_block.number + 1
            },
        };

        trace!(target: "db", "canonize {:?}", new_best_block);

        let mut update = DBTransaction::new();
        update.insert(KeyValue::BlockHash(
            new_best_block.number,
            new_best_block.hash.clone(),
        ));
        update.insert(KeyValue::BlockNumber(
            new_best_block.hash.clone(),
            new_best_block.number,
        ));
        update.insert(KeyValue::Meta(
            KEY_BEST_BLOCK_HASH,
            serialize(&new_best_block.hash),
        ));
        update.insert(KeyValue::Meta(
            KEY_BEST_BLOCK_NUMBER,
            serialize(&new_best_block.number),
        ));

        self.db.write(update).map_err(Error::DatabaseError)?;
        *best_block = new_best_block;
        Ok(())
    }

    pub fn decanonize(&self) -> Result<H256, Error> {
        let mut best_block = self.best_block.write();
        let block = match self.block(best_block.hash.clone().into()) {
            Some(block) => block,
            None => {
                error!(target: "db", "Block is not found during decanonization: {}", best_block.hash.reversed());
                return Err(Error::CannotDecanonize);
            }
        };
        let block_number = best_block.number;
        let block_hash = best_block.hash.clone();

        let new_best_block = BestBlock {
            hash: block.header.raw.previous_header_hash.clone(),
            number: if best_block.number > 0 {
                best_block.number - 1
            } else {
                assert!(block.header.raw.previous_header_hash.is_zero());
                0
            },
        };

        trace!(target: "db", "decanonize, new best: {:?}", new_best_block);

        let mut update = DBTransaction::new();
        update.delete(Key::BlockHash(block_number));
        update.delete(Key::BlockNumber(block_hash.clone()));
        update.insert(KeyValue::Meta(
            KEY_BEST_BLOCK_HASH,
            serialize(&new_best_block.hash),
        ));
        update.insert(KeyValue::Meta(
            KEY_BEST_BLOCK_NUMBER,
            serialize(&new_best_block.number),
        ));

        self.db.write(update).map_err(Error::DatabaseError)?;
        *best_block = new_best_block;
        Ok(block_hash)
    }

    fn get(&self, key: Key) -> Option<Value> {
        self.db
            .get(&key)
            .expect("db value to be fine")
            .into_option()
    }

    fn resolve_hash(&self, block_ref: BlockRef) -> Option<H256> {
        match block_ref {
            BlockRef::Number(n) => self.block_hash(n),
            BlockRef::Hash(h) => Some(h),
        }
    }
}

impl<T> BlockHeaderProvider for BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    fn block_header_bytes(&self, block_ref: BlockRef) -> Option<Bytes> {
        self.block_header(block_ref)
            .map(|header| serialize(&header.raw))
    }

    fn block_header(&self, block_ref: BlockRef) -> Option<IndexedBlockHeader> {
        self.resolve_hash(block_ref).and_then(|block_hash| {
            self.get(Key::Block(block_hash.clone()))
                .and_then(Value::as_block)
                .map(|block| IndexedBlockHeader::new(block_hash, block.block_header))
        })
    }
}

impl<T> BlockProvider for BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    fn block_number(&self, hash: &H256) -> Option<u32> {
        self.get(Key::BlockNumber(hash.clone()))
            .and_then(Value::as_block_number)
    }

    fn block_hash(&self, number: u32) -> Option<H256> {
        self.get(Key::BlockHash(number))
            .and_then(Value::as_block_hash)
    }

    fn block(&self, block_ref: BlockRef) -> Option<IndexedBlock> {
        self.resolve_hash(block_ref).and_then(|block_hash| {
            self.get(Key::Block(block_hash.clone()))
                .and_then(Value::as_block)
                .map(|block| {
                    IndexedBlock::new(
                        IndexedBlockHeader::new(block_hash, block.block_header),
                        block.proof,
                    )
                })
        })
    }

    fn contains_block(&self, block_ref: BlockRef) -> bool {
        self.resolve_hash(block_ref)
            .and_then(|hash| self.get(Key::Block(hash)))
            .is_some()
    }
}

impl<T> BlockChain for BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    fn insert(&self, block: IndexedBlock) -> Result<(), Error> {
        BlockChainDatabase::insert(self, block)
    }

    fn rollback_best(&self) -> Result<H256, Error> {
        BlockChainDatabase::rollback_best(self)
    }

    fn canonize(&self, block_hash: &H256) -> Result<(), Error> {
        BlockChainDatabase::canonize(self, block_hash)
    }

    fn decanonize(&self) -> Result<H256, Error> {
        BlockChainDatabase::decanonize(self)
    }

    fn block_origin(&self, header: &IndexedBlockHeader) -> Result<BlockOrigin, Error> {
        BlockChainDatabase::block_origin(self, header)
    }
}

impl<T> Forkable for BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    fn fork<'a>(&'a self, side_chain: SideChainOrigin) -> Result<Box<dyn ForkChain + 'a>, Error> {
        BlockChainDatabase::fork(self, side_chain).map(|fork_chain| {
            let boxed: Box<dyn ForkChain> = Box::new(fork_chain);
            boxed
        })
    }

    fn switch_to_fork<'a>(&self, fork: Box<dyn ForkChain + 'a>) -> Result<(), Error> {
        let mut best_block = self.best_block.write();
        *best_block = fork.store().best_block();
        fork.flush()
    }
}

impl<T> CanonStore for BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    fn as_store(&self) -> &dyn Store {
        &*self
    }
}

impl<T> Store for BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    fn best_block(&self) -> BestBlock {
        BlockChainDatabase::best_block(self)
    }

    /// get best header
    fn best_header(&self) -> IndexedBlockHeader {
        self.block_header(self.best_block().hash.into())
            .expect("best block header should be in db; qed")
    }

    /// get blockchain difficulty
    fn difficulty(&self) -> f64 {
        self.best_header().raw.bits.to_f64()
    }
}

impl<T> ConfigStore for BlockChainDatabase<T>
where
    T: KeyValueDatabase,
{
    // TODO:
    // + get something
    // + set something
}
