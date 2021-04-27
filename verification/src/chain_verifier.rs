//! RandChain chain verifier

use accept_chain::ChainAcceptor;
use canon::CanonBlock;
use chain::{BlockHeader, IndexedBlock, IndexedBlockHeader};
use error::Error;
use hash::H256;
use network::Network;
use storage::{BlockHeaderProvider, BlockOrigin, SharedStore};
use verify_chain::ChainVerifier;
use verify_header::HeaderVerifier;
use {VerificationLevel, Verify};

pub struct BackwardsCompatibleChainVerifier {
    store: SharedStore,
    network: Network,
}

impl BackwardsCompatibleChainVerifier {
    pub fn new(store: SharedStore, network: Network) -> Self {
        BackwardsCompatibleChainVerifier {
            store: store,
            network: network,
        }
    }

    fn verify_block(
        &self,
        verification_level: VerificationLevel,
        block: &IndexedBlock,
    ) -> Result<(), Error> {
        if verification_level == VerificationLevel::NoVerification {
            return Ok(());
        }

        let current_time = ::time::get_time().sec as u32;
        // first run pre-verification
        let chain_verifier = ChainVerifier::new(block, self.network, current_time);
        chain_verifier.check()?;

        assert_eq!(
            Some(self.store.best_block().hash),
            self.store.block_hash(self.store.best_block().number)
        );
        let block_origin = self.store.block_origin(&block.header)?;
        trace!(
            target: "verification",
            "verify_block: {:?} best_block: {:?} block_origin: {:?}",
            block.hash().reversed(),
            self.store.best_block(),
            block_origin,
        );

        let canon_block = CanonBlock::new(block);
        match block_origin {
            BlockOrigin::KnownBlock => {
                // there should be no known blocks at this point
                unreachable!();
            }
            // TODO:
            BlockOrigin::CanonChain { block_number } => {
                let header_provider = self.store.as_store().as_block_header_provider();
                let chain_acceptor =
                    ChainAcceptor::new(header_provider, &self.network, canon_block, block_number);
                chain_acceptor.check()?;
            }
            BlockOrigin::SideChain(origin) => {
                let block_number = origin.block_number;
                let fork = self.store.fork(origin)?;
                let header_provider = fork.store().as_block_header_provider();
                let chain_acceptor =
                    ChainAcceptor::new(header_provider, &self.network, canon_block, block_number);
                chain_acceptor.check()?;
            }
            BlockOrigin::SideChainBecomingCanonChain(origin) => {
                let block_number = origin.block_number;
                let fork = self.store.fork(origin)?;
                let header_provider = fork.store().as_block_header_provider();
                let chain_acceptor =
                    ChainAcceptor::new(header_provider, &self.network, canon_block, block_number);
                chain_acceptor.check()?;
            }
        };

        assert_eq!(
            Some(self.store.best_block().hash),
            self.store.block_hash(self.store.best_block().number)
        );
        Ok(())
    }

    pub fn verify_block_header(
        &self,
        _block_header_provider: &dyn BlockHeaderProvider,
        hash: &H256,
        header: &BlockHeader,
    ) -> Result<(), Error> {
        // let's do only preverifcation
        // TODO: full verification
        let current_time = ::time::get_time().sec as u32;
        let header = IndexedBlockHeader::new(hash.clone(), header.clone());
        let header_verifier = HeaderVerifier::new(&header, self.network, current_time);
        header_verifier.check()
    }
}

impl Verify for BackwardsCompatibleChainVerifier {
    fn verify(&self, level: VerificationLevel, block: &IndexedBlock) -> Result<(), Error> {
        let result = self.verify_block(level, block);
        trace!(
            // target: "verification", "Block {} (transactions: {}) verification finished. Result {:?}",
            target: "verification", "Block {} verification finished. Result {:?}",
            block.hash().to_reversed_str(),
            // block.transactions.len(),
            result,
        );
        result
    }
}

#[cfg(test)]
mod tests {
    extern crate test_data;

    use super::BackwardsCompatibleChainVerifier as ChainVerifier;
    use db::BlockChainDatabase;
    use network::Network;
    use std::sync::Arc;
    use storage::Error as DBError;
    use {Error, VerificationLevel, Verify};

    #[test]
    fn verify_orphan() {
        let storage = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let b2 = test_data::block_h2().into();
        let verifier = ChainVerifier::new(storage, Network::Unitest);
        assert_eq!(
            Err(Error::Database(DBError::UnknownParent)),
            verifier.verify(VerificationLevel::Full, &b2)
        );
    }

    #[test]
    fn verify_smoky() {
        let storage = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let b1 = test_data::block_h1();
        let verifier = ChainVerifier::new(storage, Network::Unitest);
        assert!(verifier.verify(VerificationLevel::Full, &b1.into()).is_ok());
    }
}
