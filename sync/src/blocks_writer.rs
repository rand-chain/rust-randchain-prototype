use super::Error;
use chain;
use network::Network;
use parking_lot::Mutex;
use primitives::hash::H256;
use std::collections::VecDeque;
use std::sync::Arc;
use storage;
use synchronization_chain::Chain;
use synchronization_verifier::{
    BlockVerificationSink, SyncVerifier, VerificationSink, VerificationTask, Verifier,
};
use types::StorageRef;
use utils::OrphanBlocksPool;
use VerificationParameters;

/// Maximum number of orphaned in-memory blocks
pub const MAX_ORPHANED_BLOCKS: usize = 1024;

/// Synchronous block writer
pub struct BlocksWriter {
    /// Blocks storage
    storage: StorageRef,
    /// Orphaned blocks pool
    orphaned_blocks_pool: OrphanBlocksPool,
    /// Blocks verifier
    verifier: SyncVerifier<BlocksWriterSink>,
    /// Verification events receiver
    sink: Arc<Mutex<BlocksWriterSinkData>>,
}

/// Verification events receiver
struct BlocksWriterSink {
    /// Reference to blocks writer data
    data: Arc<Mutex<BlocksWriterSinkData>>,
}

/// Blocks writer data
struct BlocksWriterSinkData {
    /// Synchronization chain.
    chain: Chain,
    /// Last verification error
    err: Option<Error>,
}

impl BlocksWriter {
    /// Create new synchronous blocks writer
    pub fn new(
        storage: StorageRef,
        network: Network,
        verification_params: VerificationParameters,
    ) -> BlocksWriter {
        let sink_data = Arc::new(Mutex::new(BlocksWriterSinkData::new(storage.clone())));
        let sink = Arc::new(BlocksWriterSink::new(sink_data.clone()));
        let verifier = SyncVerifier::new(network, storage.clone(), sink, verification_params);
        BlocksWriter {
            storage: storage,
            orphaned_blocks_pool: OrphanBlocksPool::new(),
            verifier: verifier,
            sink: sink_data,
        }
    }

    /// Append new block
    pub fn append_block(&mut self, block: chain::IndexedBlock) -> Result<(), Error> {
        // do not append block if it is already there
        if self
            .storage
            .contains_block(storage::BlockRef::Hash(block.hash().clone()))
        {
            return Ok(());
        }

        // verify && insert only if parent block is already in the storage
        if !self.storage.contains_block(storage::BlockRef::Hash(
            block.header.raw.previous_header_hash.clone(),
        )) {
            self.orphaned_blocks_pool.insert_orphaned_block(block);
            // we can't hold many orphaned blocks in memory during import
            if self.orphaned_blocks_pool.len() > MAX_ORPHANED_BLOCKS {
                return Err(Error::TooManyOrphanBlocks);
            }
            return Ok(());
        }

        // verify && insert block && all its orphan children
        let mut verification_queue: VecDeque<chain::IndexedBlock> = self
            .orphaned_blocks_pool
            .remove_blocks_for_parent(block.hash());
        verification_queue.push_front(block);
        while let Some(block) = verification_queue.pop_front() {
            self.verifier.verify_block(block);
            if let Some(err) = self.sink.lock().error() {
                return Err(err);
            }
        }

        Ok(())
    }
}

impl BlocksWriterSink {
    /// Create new verification events receiver
    pub fn new(data: Arc<Mutex<BlocksWriterSinkData>>) -> Self {
        BlocksWriterSink { data: data }
    }
}

impl BlocksWriterSinkData {
    /// Create new blocks writer data
    pub fn new(storage: StorageRef) -> Self {
        BlocksWriterSinkData {
            chain: Chain::new(storage),
            err: None,
        }
    }

    /// Take last verification error
    pub fn error(&mut self) -> Option<Error> {
        self.err.take()
    }
}

impl VerificationSink for BlocksWriterSink {}

impl BlockVerificationSink for BlocksWriterSink {
    fn on_block_verification_success(
        &self,
        block: chain::IndexedBlock,
    ) -> Option<Vec<VerificationTask>> {
        let mut data = self.data.lock();
        if let Err(err) = data.chain.insert_best_block(block) {
            data.err = Some(Error::Database(err));
        }

        None
    }

    fn on_block_verification_error(&self, err: &str, _hash: &H256) {
        self.data.lock().err = Some(Error::Verification(err.into()));
    }
}

#[cfg(test)]
mod tests {
    extern crate test_data;

    use super::super::Error;
    use super::{BlocksWriter, MAX_ORPHANED_BLOCKS};
    use db::BlockChainDatabase;
    use network::Network;
    use std::sync::Arc;
    use verification::VerificationLevel;
    use VerificationParameters;

    fn default_verification_params() -> VerificationParameters {
        VerificationParameters {
            verification_level: VerificationLevel::Full,
            verification_edge: 0u8.into(),
        }
    }

    #[test]
    fn blocks_writer_appends_blocks() {
        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let mut blocks_target =
            BlocksWriter::new(db.clone(), Network::Testnet, default_verification_params());
        blocks_target
            .append_block(test_data::block_h1().into())
            .expect("Expecting no error");
        assert_eq!(db.best_block().number, 1);
    }

    #[test]
    fn blocks_writer_verification_error() {
        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let blocks =
            test_data::build_n_empty_blocks_from_genesis((MAX_ORPHANED_BLOCKS + 2) as u32, 1);
        let mut blocks_target =
            BlocksWriter::new(db.clone(), Network::Testnet, default_verification_params());
        for (index, block) in blocks.into_iter().skip(1).enumerate() {
            match blocks_target.append_block(block.into()) {
                Err(Error::TooManyOrphanBlocks) if index == MAX_ORPHANED_BLOCKS => (),
                Ok(_) if index != MAX_ORPHANED_BLOCKS => (),
                _ => panic!("unexpected"),
            }
        }
        assert_eq!(db.best_block().number, 0);
    }

    #[test]
    fn blocks_writer_out_of_order_block() {
        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let mut blocks_target =
            BlocksWriter::new(db.clone(), Network::Testnet, default_verification_params());

        let wrong_block = test_data::block_builder()
            .header()
            .parent(test_data::genesis().hash())
            .build()
            .build();
        match blocks_target.append_block(wrong_block.into()).unwrap_err() {
            Error::Verification(_) => (),
            _ => panic!("Unexpected error"),
        };
        assert_eq!(db.best_block().number, 0);
    }

    #[test]
    fn blocks_writer_append_to_existing_db() {
        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));

        let mut blocks_target =
            BlocksWriter::new(db.clone(), Network::Testnet, default_verification_params());

        assert!(blocks_target
            .append_block(test_data::genesis().into())
            .is_ok());
        assert_eq!(db.best_block().number, 0);

        assert!(blocks_target
            .append_block(test_data::block_h1().into())
            .is_ok());
        assert_eq!(db.best_block().number, 1);
    }

    #[test]
    fn blocks_write_able_to_reorganize() {
        // (1) b0 ---> (2) b1
        //        \--> (3) b2 ---> (4 - reorg) b3
        let b0 = test_data::block_builder().header().build().build();
        let b1 = test_data::block_builder()
            .header()
            .iterations(1)
            .parent(b0.hash())
            .build()
            .build();
        let b2 = test_data::block_builder()
            .header()
            .iterations(2)
            .parent(b0.hash())
            .build()
            .build();
        let b3 = test_data::block_builder()
            .header()
            .parent(b2.hash())
            .build()
            .build();

        let db = Arc::new(BlockChainDatabase::init_test_chain(vec![b0.into()]));
        let mut blocks_target = BlocksWriter::new(
            db.clone(),
            Network::Testnet,
            VerificationParameters {
                verification_level: VerificationLevel::NoVerification,
                verification_edge: 0u8.into(),
            },
        );
        assert_eq!(blocks_target.append_block(b1.into()), Ok(()));
        assert_eq!(blocks_target.append_block(b2.into()), Ok(()));
        assert_eq!(blocks_target.append_block(b3.into()), Ok(()));
    }
}
