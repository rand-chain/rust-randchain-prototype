use primitives::hash::H256;
use utils::{KnownHashFilter, KnownHashType};

/// Filter, which controls data relayed over connection.
#[derive(Debug, Default)]
pub struct ConnectionFilter {
    /// Known hashes filter
    known_hash_filter: KnownHashFilter,
}

impl ConnectionFilter {
    /// Add known item hash
    pub fn hash_known_as(&mut self, hash: H256, hash_type: KnownHashType) {
        self.known_hash_filter.insert(hash, hash_type);
    }

    /// Is item with given hash && type is known by peer
    pub fn is_hash_known_as(&self, hash: &H256, hash_type: KnownHashType) -> bool {
        self.known_hash_filter.contains(hash, hash_type)
    }

    /// Check if block should be sent to this connection
    pub fn filter_block(&self, block_hash: &H256) -> bool {
        self.known_hash_filter.filter_block(block_hash)
    }
}

#[cfg(test)]
pub mod tests {
    extern crate test_data;

    use super::ConnectionFilter;
    use utils::KnownHashType;

    #[test]
    fn filter_default_accepts_block() {
        assert!(ConnectionFilter::default().filter_block(&test_data::genesis().hash()));
    }

    #[test]
    fn filter_rejects_block_known() {
        let mut filter = ConnectionFilter::default();
        filter.hash_known_as(test_data::block_h1().hash(), KnownHashType::Block);
        assert!(!filter.filter_block(&test_data::block_h1().hash()));
        assert!(filter.filter_block(&test_data::genesis().hash()));
    }
}
