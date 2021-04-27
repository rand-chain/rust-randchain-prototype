use chain::{IndexedBlock, IndexedBlockHeader};
use primitives::bytes::Bytes;
use primitives::hash::H256;
use std::collections::{HashSet, VecDeque};
use std::fmt;
use storage;
use types::{BlockHeight, StorageRef};
use utils::{BestHeadersChain, BestHeadersChainInformation, HashPosition, HashQueueChain};

/// Index of 'verifying' queue
const VERIFYING_QUEUE: usize = 0;
/// Index of 'requested' queue
const REQUESTED_QUEUE: usize = 1;
/// Index of 'scheduled' queue
const SCHEDULED_QUEUE: usize = 2;
/// Number of hash queues
const NUMBER_OF_QUEUES: usize = 3;

/// Block insertion result
#[derive(Default, PartialEq)]
pub struct BlockInsertionResult {
    /// Hashes of blocks, which were canonized during this insertion procedure. Order matters
    pub canonized_blocks_hashes: Vec<H256>,
}

impl fmt::Debug for BlockInsertionResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BlockInsertionResult")
            .field(
                "canonized_blocks_hashes",
                &self
                    .canonized_blocks_hashes
                    .iter()
                    .map(H256::reversed)
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl BlockInsertionResult {
    #[cfg(test)]
    pub fn with_canonized_blocks(canonized_blocks_hashes: Vec<H256>) -> Self {
        BlockInsertionResult {
            canonized_blocks_hashes: canonized_blocks_hashes,
        }
    }
}

/// Block synchronization state
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BlockState {
    /// Block is unknown
    Unknown,
    /// Scheduled for requesting
    Scheduled,
    /// Requested from peers
    Requested,
    /// Currently verifying
    Verifying,
    /// In storage
    Stored,
    /// This block has been marked as dead-end block
    DeadEnd,
}

/// Synchronization chain information
pub struct Information {
    /// Number of blocks hashes currently scheduled for requesting
    pub scheduled: BlockHeight,
    /// Number of blocks hashes currently requested from peers
    pub requested: BlockHeight,
    /// Number of blocks currently verifying
    pub verifying: BlockHeight,
    /// Number of blocks in the storage
    pub stored: BlockHeight,
    /// Information on headers chain
    pub headers: BestHeadersChainInformation,
}

/// Blockchain from synchroniation point of view, consisting of:
/// 1) all blocks from the `storage` [oldest blocks]
/// 2) all blocks currently verifying by `verification_queue`
/// 3) all blocks currently requested from peers
/// 4) all blocks currently scheduled for requesting [newest blocks]
pub struct Chain {
    /// Genesis block hash (stored for optimizations)
    genesis_block_hash: H256,
    /// Best storage block (stored for optimizations)
    best_storage_block: storage::BestBlock,
    /// Local blocks storage
    storage: StorageRef,
    /// In-memory queue of blocks hashes
    hash_chain: HashQueueChain,
    /// In-memory queue of blocks headers
    headers_chain: BestHeadersChain,
    /// Blocks that have been marked as dead-ends
    dead_end_blocks: HashSet<H256>,
}

impl BlockState {
    pub fn from_queue_index(queue_index: usize) -> BlockState {
        match queue_index {
            SCHEDULED_QUEUE => BlockState::Scheduled,
            REQUESTED_QUEUE => BlockState::Requested,
            VERIFYING_QUEUE => BlockState::Verifying,
            _ => panic!("Unsupported queue_index: {}", queue_index),
        }
    }

    pub fn to_queue_index(&self) -> usize {
        match *self {
            BlockState::Scheduled => SCHEDULED_QUEUE,
            BlockState::Requested => REQUESTED_QUEUE,
            BlockState::Verifying => VERIFYING_QUEUE,
            _ => panic!("Unsupported queue: {:?}", self),
        }
    }
}

impl Chain {
    /// Create new `Chain` with given storage
    pub fn new(storage: StorageRef) -> Self {
        // we only work with storages with genesis block
        let genesis_block_hash = storage
            .block_hash(0)
            .expect("storage with genesis block is required");
        let best_storage_block = storage.best_block();
        let best_storage_block_hash = best_storage_block.hash.clone();

        Chain {
            genesis_block_hash: genesis_block_hash,
            best_storage_block: best_storage_block,
            storage: storage,
            hash_chain: HashQueueChain::with_number_of_queues(NUMBER_OF_QUEUES),
            headers_chain: BestHeadersChain::new(best_storage_block_hash),
            dead_end_blocks: HashSet::new(),
        }
    }

    /// Get information on current blockchain state
    pub fn information(&self) -> Information {
        Information {
            scheduled: self.hash_chain.len_of(SCHEDULED_QUEUE),
            requested: self.hash_chain.len_of(REQUESTED_QUEUE),
            verifying: self.hash_chain.len_of(VERIFYING_QUEUE),
            stored: self.best_storage_block.number + 1,
            headers: self.headers_chain.information(),
        }
    }

    /// Get storage
    pub fn storage(&self) -> StorageRef {
        self.storage.clone()
    }

    /// Get number of blocks in given state
    pub fn length_of_blocks_state(&self, state: BlockState) -> BlockHeight {
        match state {
            BlockState::Stored => self.best_storage_block.number + 1,
            _ => self.hash_chain.len_of(state.to_queue_index()),
        }
    }

    /// Get n best blocks of given state
    pub fn best_n_of_blocks_state(&self, state: BlockState, n: BlockHeight) -> Vec<H256> {
        match state {
            BlockState::Scheduled | BlockState::Requested | BlockState::Verifying => {
                self.hash_chain.front_n_at(state.to_queue_index(), n)
            }
            _ => unreachable!("must be checked by caller"),
        }
    }

    /// Get best block
    pub fn best_block(&self) -> storage::BestBlock {
        match self.hash_chain.back() {
            Some(hash) => storage::BestBlock {
                number: self.best_storage_block.number + self.hash_chain.len(),
                hash: hash.clone(),
            },
            None => self.best_storage_block.clone(),
        }
    }

    /// Get best storage block
    pub fn best_storage_block(&self) -> storage::BestBlock {
        self.best_storage_block.clone()
    }

    /// Get best block header
    pub fn best_block_header(&self) -> storage::BestBlock {
        let headers_chain_information = self.headers_chain.information();
        if headers_chain_information.best == 0 {
            return self.best_storage_block();
        }
        storage::BestBlock {
            number: self.best_storage_block.number + headers_chain_information.best,
            hash: self
                .headers_chain
                .at(headers_chain_information.best - 1)
                .expect("got this index above; qed")
                .hash,
        }
    }

    /// Get block header by hash
    pub fn block_hash(&self, number: BlockHeight) -> Option<H256> {
        if number <= self.best_storage_block.number {
            self.storage.block_hash(number)
        } else {
            // we try to keep these in order, but they are probably not
            self.hash_chain.at(number - self.best_storage_block.number)
        }
    }

    /// Get block number by hash
    pub fn block_number(&self, hash: &H256) -> Option<BlockHeight> {
        if let Some(number) = self.storage.block_number(hash) {
            return Some(number);
        }
        self.headers_chain
            .height(hash)
            .map(|p| self.best_storage_block.number + p + 1)
    }

    /// Get block header by number
    pub fn block_header_by_number(&self, number: BlockHeight) -> Option<IndexedBlockHeader> {
        if number <= self.best_storage_block.number {
            self.storage.block_header(storage::BlockRef::Number(number))
        } else {
            self.headers_chain
                .at(number - self.best_storage_block.number)
        }
    }

    /// Get block header by hash
    pub fn block_header_by_hash(&self, hash: &H256) -> Option<IndexedBlockHeader> {
        if let Some(header) = self.storage.block_header(storage::BlockRef::Hash(*hash)) {
            return Some(header);
        }
        self.headers_chain.by_hash(hash)
    }

    /// Get block state
    pub fn block_state(&self, hash: &H256) -> BlockState {
        match self.hash_chain.contains_in(hash) {
            Some(queue_index) => BlockState::from_queue_index(queue_index),
            None => {
                if self.storage.contains_block(storage::BlockRef::Hash(*hash)) {
                    BlockState::Stored
                } else if self.dead_end_blocks.contains(hash) {
                    BlockState::DeadEnd
                } else {
                    BlockState::Unknown
                }
            }
        }
    }

    /// Prepare block locator hashes, as described in protocol documentation:
    /// https://en.bitcoin.it/wiki/Protocol_documentation#getblocks
    /// When there are forked blocks in the queue, this method can result in
    /// mixed block locator hashes ([0 - from fork1, 1 - from fork2, 2 - from fork1]).
    /// Peer will respond with blocks of fork1 || fork2 => we could end up in some side fork
    /// To resolve this, after switching to saturated state, we will also ask all peers for inventory.
    pub fn block_locator_hashes(&self) -> Vec<H256> {
        let mut block_locator_hashes: Vec<H256> = Vec::new();

        // calculate for hash_queue
        let (local_index, step) = self.block_locator_hashes_for_queue(&mut block_locator_hashes);

        // calculate for storage
        let storage_index = if self.best_storage_block.number < local_index {
            0
        } else {
            self.best_storage_block.number - local_index
        };
        self.block_locator_hashes_for_storage(storage_index, step, &mut block_locator_hashes);
        block_locator_hashes
    }

    /// Schedule blocks hashes for requesting
    pub fn schedule_blocks_headers(&mut self, headers: Vec<IndexedBlockHeader>) {
        self.hash_chain.push_back_n_at(
            SCHEDULED_QUEUE,
            headers.iter().map(|h| h.hash.clone()).collect(),
        );
        self.headers_chain.insert_n(headers);
    }

    /// Moves n blocks from scheduled queue to requested queue
    pub fn request_blocks_hashes(&mut self, n: BlockHeight) -> Vec<H256> {
        let scheduled = self.hash_chain.pop_front_n_at(SCHEDULED_QUEUE, n);
        self.hash_chain
            .push_back_n_at(REQUESTED_QUEUE, scheduled.clone());
        scheduled
    }

    /// Add block to verifying queue
    pub fn verify_block(&mut self, header: IndexedBlockHeader) {
        // insert header to the in-memory chain in case when it is not already there (non-headers-first sync)
        self.hash_chain
            .push_back_at(VERIFYING_QUEUE, header.hash.clone());
        self.headers_chain.insert(header);
    }

    /// Add blocks to verifying queue
    pub fn verify_blocks(&mut self, blocks: Vec<IndexedBlockHeader>) {
        for block in blocks {
            self.verify_block(block);
        }
    }

    /// Moves n blocks from requested queue to verifying queue
    #[cfg(test)]
    pub fn verify_blocks_hashes(&mut self, n: BlockHeight) -> Vec<H256> {
        let requested = self.hash_chain.pop_front_n_at(REQUESTED_QUEUE, n);
        self.hash_chain
            .push_back_n_at(VERIFYING_QUEUE, requested.clone());
        requested
    }

    /// Mark this block as dead end, so these tasks won't be synchronized
    pub fn mark_dead_end_block(&mut self, hash: &H256) {
        self.dead_end_blocks.insert(*hash);
    }

    /// Insert new best block to storage
    pub fn insert_best_block(
        &mut self,
        block: IndexedBlock,
    ) -> Result<BlockInsertionResult, storage::Error> {
        assert_eq!(
            Some(self.storage.best_block().hash),
            self.storage.block_hash(self.storage.best_block().number)
        );
        let block_origin = self.storage.block_origin(&block.header)?;
        trace!(target: "sync", "insert_best_block {:?} origin: {:?}", block.hash().reversed(), block_origin);
        match block_origin {
            storage::BlockOrigin::KnownBlock => {
                // there should be no known blocks at this point
                unreachable!();
            }
            // case 1: block has been added to the main branch
            storage::BlockOrigin::CanonChain { .. } => {
                self.storage.insert(block.clone())?;
                self.storage.canonize(block.hash())?;

                // remember new best block hash
                self.best_storage_block = self.storage.as_store().best_block();

                // remove inserted block + handle possible reorganization in headers chain
                // TODO: mk, not sure if we need both of those params
                self.headers_chain
                    .block_inserted_to_storage(block.hash(), &self.best_storage_block.hash);

                // double check
                assert_eq!(self.best_storage_block.hash, block.hash().clone());

                Ok(BlockInsertionResult {
                    canonized_blocks_hashes: vec![*block.hash()],
                })
            }
            // case 2: block has been added to the side branch with reorganization to this branch
            storage::BlockOrigin::SideChainBecomingCanonChain(origin) => {
                let fork = self.storage.fork(origin.clone())?;
                fork.store().insert(block.clone())?;
                fork.store().canonize(block.hash())?;
                self.storage.switch_to_fork(fork)?;

                // remember new best block hash
                self.best_storage_block = self.storage.best_block();

                // remove inserted block + handle possible reorganization in headers chain
                // TODO: mk, not sure if we need both of those params
                self.headers_chain
                    .block_inserted_to_storage(block.hash(), &self.best_storage_block.hash);

                let mut canonized_blocks_hashes = origin.canonized_route.clone();
                canonized_blocks_hashes.push(*block.hash());
                let result = BlockInsertionResult {
                    canonized_blocks_hashes: canonized_blocks_hashes,
                };

                trace!(target: "sync", "result: {:?}", result);

                Ok(result)
            }
            // case 3: block has been added to the side branch without reorganization to this branch
            storage::BlockOrigin::SideChain(_origin) => {
                let block_hash = block.hash().clone();
                self.storage.insert(block)?;

                // remove inserted block + handle possible reorganization in headers chain
                // TODO: mk, not sure if it's needed here at all
                self.headers_chain
                    .block_inserted_to_storage(&block_hash, &self.best_storage_block.hash);

                // no transactions were accepted
                // no transactions to reverify
                Ok(BlockInsertionResult::default())
            }
        }
    }

    /// Forget in-memory block
    pub fn forget_block(&mut self, hash: &H256) -> HashPosition {
        self.headers_chain.remove(hash);
        self.forget_block_leave_header(hash)
    }

    /// Forget in-memory blocks
    pub fn forget_blocks(&mut self, hashes: &[H256]) {
        for hash in hashes {
            self.forget_block(hash);
        }
    }

    /// Forget in-memory block, but leave its header in the headers_chain (orphan queue)
    pub fn forget_block_leave_header(&mut self, hash: &H256) -> HashPosition {
        match self.hash_chain.remove_at(VERIFYING_QUEUE, hash) {
            HashPosition::Missing => match self.hash_chain.remove_at(REQUESTED_QUEUE, hash) {
                HashPosition::Missing => self.hash_chain.remove_at(SCHEDULED_QUEUE, hash),
                position => position,
            },
            position => position,
        }
    }

    /// Forget in-memory blocks, but leave their headers in the headers_chain (orphan queue)
    pub fn forget_blocks_leave_header(&mut self, hashes: &[H256]) {
        for hash in hashes {
            self.forget_block_leave_header(hash);
        }
    }

    /// Forget in-memory block by hash if it is currently in given state
    #[cfg(test)]
    pub fn forget_block_with_state(&mut self, hash: &H256, state: BlockState) -> HashPosition {
        self.headers_chain.remove(hash);
        self.forget_block_with_state_leave_header(hash, state)
    }

    /// Forget in-memory block by hash if it is currently in given state
    pub fn forget_block_with_state_leave_header(
        &mut self,
        hash: &H256,
        state: BlockState,
    ) -> HashPosition {
        self.hash_chain.remove_at(state.to_queue_index(), hash)
    }

    /// Forget in-memory block by hash.
    /// Also forget all its known children.
    pub fn forget_block_with_children(&mut self, hash: &H256) {
        let mut removal_stack: VecDeque<H256> = VecDeque::new();
        let mut removal_queue: VecDeque<H256> = VecDeque::new();
        removal_queue.push_back(*hash);

        // remove in reverse order to minimize headers operations
        while let Some(hash) = removal_queue.pop_front() {
            removal_queue.extend(self.headers_chain.children(&hash));
            removal_stack.push_back(hash);
        }
        while let Some(hash) = removal_stack.pop_back() {
            self.forget_block(&hash);
        }
    }

    /// Forget all blocks with given state
    pub fn forget_all_blocks_with_state(&mut self, state: BlockState) {
        let hashes = self.hash_chain.remove_all_at(state.to_queue_index());
        self.headers_chain.remove_n(hashes);
    }

    /// Calculate block locator hashes for hash queue
    fn block_locator_hashes_for_queue(&self, hashes: &mut Vec<H256>) -> (BlockHeight, BlockHeight) {
        let queue_len = self.hash_chain.len();
        if queue_len == 0 {
            return (0, 1);
        }

        let mut index = queue_len - 1;
        let mut step = 1u32;
        loop {
            let block_hash = self.hash_chain[index].clone();
            hashes.push(block_hash);

            if hashes.len() >= 10 {
                step <<= 1;
            }
            if index < step {
                return (step - index - 1, step);
            }
            index -= step;
        }
    }

    /// Calculate block locator hashes for storage
    fn block_locator_hashes_for_storage(
        &self,
        mut index: BlockHeight,
        mut step: BlockHeight,
        hashes: &mut Vec<H256>,
    ) {
        loop {
            let block_hash = self
                .storage
                .block_hash(index)
                .expect("private function; index calculated in `block_locator_hashes`; qed");
            hashes.push(block_hash);

            if hashes.len() >= 10 {
                step <<= 1;
            }
            if index < step {
                // always include genesis hash
                if index != 0 {
                    hashes.push(self.genesis_block_hash.clone())
                }

                break;
            }
            index -= step;
        }
    }
}

impl storage::BlockHeaderProvider for Chain {
    fn block_header_bytes(&self, block_ref: storage::BlockRef) -> Option<Bytes> {
        use ser::serialize;
        self.block_header(block_ref).map(|h| serialize(&h.raw))
    }

    fn block_header(&self, block_ref: storage::BlockRef) -> Option<IndexedBlockHeader> {
        match block_ref {
            storage::BlockRef::Hash(hash) => self.block_header_by_hash(&hash),
            storage::BlockRef::Number(n) => self.block_header_by_number(n),
        }
    }
}

impl fmt::Debug for Information {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "[sch:{} -> req:{} -> vfy:{} -> stored: {}]",
            self.scheduled, self.requested, self.verifying, self.stored
        )
    }
}

impl fmt::Debug for Chain {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "chain: [")?;
        {
            let mut num = self.best_storage_block.number;
            writeln!(f, "\tworse(stored): {} {:?}", 0, self.storage.block_hash(0))?;
            writeln!(
                f,
                "\tbest(stored): {} {:?}",
                num,
                self.storage.block_hash(num)
            )?;

            let queues = vec![
                ("verifying", VERIFYING_QUEUE),
                ("requested", REQUESTED_QUEUE),
                ("scheduled", SCHEDULED_QUEUE),
            ];
            for (state, queue) in queues {
                let queue_len = self.hash_chain.len_of(queue);
                if queue_len != 0 {
                    writeln!(
                        f,
                        "\tworse({}): {} {:?}",
                        state,
                        num + 1,
                        self.hash_chain.front_at(queue)
                    )?;
                    num += queue_len;
                    if let Some(pre_best) = self.hash_chain.pre_back_at(queue) {
                        writeln!(f, "\tpre-best({}): {} {:?}", state, num - 1, pre_best)?;
                    }
                    writeln!(
                        f,
                        "\tbest({}): {} {:?}",
                        state,
                        num,
                        self.hash_chain.back_at(queue)
                    )?;
                }
            }
        }
        writeln!(f, "]")
    }
}

#[cfg(test)]
mod tests {
    extern crate test_data;

    use super::{BlockState, Chain};
    use chain::IndexedBlockHeader;
    use db::BlockChainDatabase;
    use primitives::hash::H256;
    use std::sync::Arc;
    use utils::HashPosition;

    #[test]
    fn chain_empty() {
        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let db_best_block = db.best_block();
        let chain = Chain::new(db.clone());
        assert_eq!(chain.information().scheduled, 0);
        assert_eq!(chain.information().requested, 0);
        assert_eq!(chain.information().verifying, 0);
        assert_eq!(chain.information().stored, 1);
        assert_eq!(chain.length_of_blocks_state(BlockState::Scheduled), 0);
        assert_eq!(chain.length_of_blocks_state(BlockState::Requested), 0);
        assert_eq!(chain.length_of_blocks_state(BlockState::Verifying), 0);
        assert_eq!(chain.length_of_blocks_state(BlockState::Stored), 1);
        assert_eq!(&chain.best_block(), &db_best_block);
        assert_eq!(chain.block_state(&db_best_block.hash), BlockState::Stored);
        assert_eq!(chain.block_state(&H256::from(0)), BlockState::Unknown);
    }

    #[test]
    fn chain_block_path() {
        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let mut chain = Chain::new(db.clone());

        // add 6 blocks to scheduled queue
        let blocks = test_data::build_n_empty_blocks_from_genesis(6, 0);
        let headers: Vec<IndexedBlockHeader> =
            blocks.into_iter().map(|b| b.block_header.into()).collect();
        let hashes: Vec<_> = headers.iter().map(|h| h.hash.clone()).collect();
        chain.schedule_blocks_headers(headers.clone());
        assert!(
            chain.information().scheduled == 6
                && chain.information().requested == 0
                && chain.information().verifying == 0
                && chain.information().stored == 1
        );

        // move 2 best blocks to requested queue
        chain.request_blocks_hashes(2);
        assert!(
            chain.information().scheduled == 4
                && chain.information().requested == 2
                && chain.information().verifying == 0
                && chain.information().stored == 1
        );
        // move 0 best blocks to requested queue
        chain.request_blocks_hashes(0);
        assert!(
            chain.information().scheduled == 4
                && chain.information().requested == 2
                && chain.information().verifying == 0
                && chain.information().stored == 1
        );
        // move 1 best blocks to requested queue
        chain.request_blocks_hashes(1);
        assert!(
            chain.information().scheduled == 3
                && chain.information().requested == 3
                && chain.information().verifying == 0
                && chain.information().stored == 1
        );

        // try to remove block 0 from scheduled queue => missing
        assert_eq!(
            chain.forget_block_with_state(&hashes[0], BlockState::Scheduled),
            HashPosition::Missing
        );
        assert!(
            chain.information().scheduled == 3
                && chain.information().requested == 3
                && chain.information().verifying == 0
                && chain.information().stored == 1
        );
        // remove blocks 0 & 1 from requested queue
        assert_eq!(
            chain.forget_block_with_state(&hashes[1], BlockState::Requested),
            HashPosition::Inside(1)
        );
        assert_eq!(
            chain.forget_block_with_state(&hashes[0], BlockState::Requested),
            HashPosition::Front
        );
        assert!(
            chain.information().scheduled == 3
                && chain.information().requested == 1
                && chain.information().verifying == 0
                && chain.information().stored == 1
        );
        // mark 0 & 1 as verifying
        chain.verify_block(headers[0].clone().into());
        chain.verify_block(headers[1].clone().into());
        assert!(
            chain.information().scheduled == 3
                && chain.information().requested == 1
                && chain.information().verifying == 2
                && chain.information().stored == 1
        );

        // mark block 0 as verified
        assert_eq!(
            chain.forget_block_with_state(&hashes[0], BlockState::Verifying),
            HashPosition::Front
        );
        assert!(
            chain.information().scheduled == 3
                && chain.information().requested == 1
                && chain.information().verifying == 1
                && chain.information().stored == 1
        );
        // insert new best block to the chain
        chain
            .insert_best_block(test_data::block_h1().into())
            .expect("Db error");
        assert!(
            chain.information().scheduled == 3
                && chain.information().requested == 1
                && chain.information().verifying == 1
                && chain.information().stored == 2
        );
        assert_eq!(db.best_block().number, 1);
    }

    #[test]
    fn chain_block_locator_hashes() {
        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let mut chain = Chain::new(db);
        let genesis_hash = chain.best_block().hash;
        assert_eq!(chain.block_locator_hashes(), vec![genesis_hash.clone()]);

        let block1 = test_data::block_h1();
        let block1_hash = block1.hash();

        chain
            .insert_best_block(block1.into())
            .expect("Error inserting new block");
        assert_eq!(
            chain.block_locator_hashes(),
            vec![block1_hash.clone(), genesis_hash.clone()]
        );

        let block2 = test_data::block_h2();
        let block2_hash = block2.hash();

        chain
            .insert_best_block(block2.into())
            .expect("Error inserting new block");
        assert_eq!(
            chain.block_locator_hashes(),
            vec![
                block2_hash.clone(),
                block1_hash.clone(),
                genesis_hash.clone()
            ]
        );

        let blocks0 = test_data::build_n_empty_blocks_from_genesis(11, 0);
        let headers0: Vec<IndexedBlockHeader> =
            blocks0.into_iter().map(|b| b.block_header.into()).collect();
        let hashes0: Vec<_> = headers0.iter().map(|h| h.hash.clone()).collect();
        chain.schedule_blocks_headers(headers0.clone());
        chain.request_blocks_hashes(10);
        chain.verify_blocks_hashes(10);

        assert_eq!(
            chain.block_locator_hashes(),
            vec![
                hashes0[10].clone(),
                hashes0[9].clone(),
                hashes0[8].clone(),
                hashes0[7].clone(),
                hashes0[6].clone(),
                hashes0[5].clone(),
                hashes0[4].clone(),
                hashes0[3].clone(),
                hashes0[2].clone(),
                hashes0[1].clone(),
                block2_hash.clone(),
                genesis_hash.clone(),
            ]
        );

        let blocks1 = test_data::build_n_empty_blocks_from(6, 0, &headers0[10].raw);
        let headers1: Vec<IndexedBlockHeader> =
            blocks1.into_iter().map(|b| b.block_header.into()).collect();
        let hashes1: Vec<_> = headers1.iter().map(|h| h.hash.clone()).collect();
        chain.schedule_blocks_headers(headers1.clone());
        chain.request_blocks_hashes(10);

        assert_eq!(
            chain.block_locator_hashes(),
            vec![
                hashes1[5].clone(),
                hashes1[4].clone(),
                hashes1[3].clone(),
                hashes1[2].clone(),
                hashes1[1].clone(),
                hashes1[0].clone(),
                hashes0[10].clone(),
                hashes0[9].clone(),
                hashes0[8].clone(),
                hashes0[7].clone(),
                hashes0[5].clone(),
                hashes0[1].clone(),
                genesis_hash.clone(),
            ]
        );

        let blocks2 = test_data::build_n_empty_blocks_from(3, 0, &headers1[5].raw);
        let headers2: Vec<IndexedBlockHeader> =
            blocks2.into_iter().map(|b| b.block_header.into()).collect();
        let hashes2: Vec<_> = headers2.iter().map(|h| h.hash.clone()).collect();
        chain.schedule_blocks_headers(headers2);

        assert_eq!(
            chain.block_locator_hashes(),
            vec![
                hashes2[2].clone(),
                hashes2[1].clone(),
                hashes2[0].clone(),
                hashes1[5].clone(),
                hashes1[4].clone(),
                hashes1[3].clone(),
                hashes1[2].clone(),
                hashes1[1].clone(),
                hashes1[0].clone(),
                hashes0[10].clone(),
                hashes0[8].clone(),
                hashes0[4].clone(),
                genesis_hash.clone(),
            ]
        );
    }
}
