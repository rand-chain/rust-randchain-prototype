use chain::IndexedBlock;
use error::Error;
use network::Network;
use verify_block::BlockVerifier;
use verify_header::HeaderVerifier;

pub struct ChainVerifier<'a> {
    pub block: BlockVerifier<'a>,
    pub header: HeaderVerifier<'a>,
}

impl<'a> ChainVerifier<'a> {
    pub fn new(block: &'a IndexedBlock, network: Network, current_time: u32) -> Self {
        trace!(target: "verification", "Block pre-verification {}", block.hash().to_reversed_str());
        ChainVerifier {
            block: BlockVerifier::new(block),
            header: HeaderVerifier::new(&block.header, network, current_time),
        }
    }

    pub fn check(&self) -> Result<(), Error> {
        self.block.check()?;
        self.header.check()?;
        Ok(())
    }
}
