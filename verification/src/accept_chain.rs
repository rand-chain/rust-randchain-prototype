use accept_block::BlockAcceptor;
use accept_header::HeaderAcceptor;
use canon::CanonBlock;
use error::Error;
use network::Network;
use storage::BlockHeaderProvider;

pub struct ChainAcceptor<'a> {
    pub block: BlockAcceptor<'a>,
    pub header: HeaderAcceptor<'a>,
}

impl<'a> ChainAcceptor<'a> {
    pub fn new(
        header_provider: &'a dyn BlockHeaderProvider,
        network: &'a Network,
        block: CanonBlock<'a>,
        height: u32,
    ) -> Self {
        trace!(target: "verification", "Block verification {}", block.hash().to_reversed_str());

        ChainAcceptor {
            block: BlockAcceptor::new(block, height, header_provider),
            header: HeaderAcceptor::new(header_provider, network, block.header(), height),
        }
    }

    pub fn check(&self) -> Result<(), Error> {
        self.block.check()?;
        self.header.check()?;
        Ok(())
    }
}
