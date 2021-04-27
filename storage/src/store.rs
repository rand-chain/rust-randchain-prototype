use chain::IndexedBlockHeader;
use std::sync::Arc;
use {BestBlock, BlockChain, BlockHeaderProvider, BlockProvider, Forkable};

pub trait CanonStore: Store + Forkable + ConfigStore {
    fn as_store(&self) -> &dyn Store;
}

/// Configuration storage interface
pub trait ConfigStore {
    // TODO:
    // + get something
    // + set something
}

/// Blockchain storage interface
pub trait Store: AsSubstore {
    /// get best block
    fn best_block(&self) -> BestBlock;

    /// get best header
    fn best_header(&self) -> IndexedBlockHeader;

    /// get blockchain difficulty
    fn difficulty(&self) -> f64;
}

/// Allows casting Arc<Store> to reference to any substore type
pub trait AsSubstore: BlockChain + BlockProvider {
    fn as_block_provider(&self) -> &dyn BlockProvider;

    fn as_block_header_provider(&self) -> &dyn BlockHeaderProvider;
}

impl<T> AsSubstore for T
where
    T: BlockChain + BlockProvider,
{
    fn as_block_provider(&self) -> &dyn BlockProvider {
        &*self
    }

    fn as_block_header_provider(&self) -> &dyn BlockHeaderProvider {
        &*self
    }
}

pub type SharedStore = Arc<dyn CanonStore + Send + Sync>;
