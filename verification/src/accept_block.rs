use canon::CanonBlock;
use error::Error;
use storage::BlockHeaderProvider;

/// Flexible verification of ordered block
pub struct BlockAcceptor<'a> {
    pub finality: BlockFinality<'a>,
}

impl<'a> BlockAcceptor<'a> {
    pub fn new(block: CanonBlock<'a>, height: u32, headers: &'a dyn BlockHeaderProvider) -> Self {
        BlockAcceptor {
            finality: BlockFinality::new(block, height, headers),
        }
    }

    pub fn check(&self) -> Result<(), Error> {
        self.finality.check()
    }
}

pub struct BlockFinality<'a> {
    block: CanonBlock<'a>,
    height: u32,
    headers: &'a dyn BlockHeaderProvider,
}

impl<'a> BlockFinality<'a> {
    fn new(block: CanonBlock<'a>, height: u32, headers: &'a dyn BlockHeaderProvider) -> Self {
        BlockFinality {
            block: block,
            height: height,
            headers: headers,
        }
    }

    fn check(&self) -> Result<(), Error> {
        // TODO:
        Ok(())
    }
}
