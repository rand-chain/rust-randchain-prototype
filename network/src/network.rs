//! randchain network

use chain::{Block, BlockHeader, IndexedBlock};
use compact::Compact;
use primitives::bigint::U256;
use primitives::hash::H256;

// TODO:
// These are the same as bitcoin as described in
// https://www.anintegratedworld.com/unravelling-the-mysterious-block-chain-magic-number/
// but we may need to design our own
const MAGIC_MAINNET: u32 = 0xD9B4BEF9;
const MAGIC_TESTNET: u32 = 0x0709110B;
const MAGIC_REGTEST: u32 = 0xDAB5BFFA;
const MAGIC_UNITEST: u32 = 0x00000000;

lazy_static! {
    static ref MAX_BITS_MAINNET: U256 =
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            .parse()
            .expect("hardcoded value should parse without errors");
    static ref MAX_BITS_TESTNET: U256 =
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            .parse()
            .expect("hardcoded value should parse without errors");
    static ref MAX_BITS_REGTEST: U256 =
        "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
            .parse()
            .expect("hardcoded value should parse without errors");
}

/// Network magic type.
pub type Magic = u32;

/// RandChain network
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Network {
    /// The original and main network for RandChain.
    Mainnet,
    /// The main RandChain testnet.
    Testnet,
    /// RandChain regtest network.
    Regtest,
    /// Testnet for unittests, proof of work difficulty is almost 0
    Unitest,
    /// Any other network. By default behaves like RandChain mainnet.
    Other(u32),
}

impl Network {
    pub fn magic(&self) -> Magic {
        match *self {
            Network::Mainnet => MAGIC_MAINNET,
            Network::Testnet => MAGIC_TESTNET,
            Network::Regtest => MAGIC_REGTEST,
            Network::Unitest => MAGIC_UNITEST,
            Network::Other(value) => value,
        }
    }

    pub fn max_bits(&self) -> U256 {
        match *self {
            Network::Mainnet | Network::Other(_) => MAX_BITS_MAINNET.clone(),
            Network::Testnet => MAX_BITS_TESTNET.clone(),
            Network::Regtest => MAX_BITS_REGTEST.clone(),
            Network::Unitest => Compact::max_value().into(),
        }
    }

    pub fn port(&self) -> u16 {
        match *self {
            Network::Mainnet | Network::Other(_) => 8333,
            Network::Testnet => 18333,
            Network::Regtest | Network::Unitest => 18444,
        }
    }

    pub fn dns_port(&self) -> u16 {
        53u16
    }

    pub fn rpc_port(&self) -> u16 {
        match *self {
            Network::Mainnet | Network::Other(_) => 8332,
            Network::Testnet => 18332,
            Network::Regtest | Network::Unitest => 18443,
        }
    }

    pub fn genesis_block(&self) -> IndexedBlock {
        match *self {
            Network::Mainnet | Network::Other(_) => {
                let blk = Block {
                    block_header: BlockHeader {
                        version: 1,
                        previous_header_hash: [0; 32].into(), // genesis_block has all-0 previous_header_hash
                        time: 4,
                        bits: 5.into(),
                        pubkey: ecvrf::VrfPk::from_bytes(&[6; 32]).unwrap(),
                        iterations: 100000,
                        randomness: rug::Integer::from(8),
                    },
                    proof: vec![],
                };
                IndexedBlock::from_raw(blk)
            }
            Network::Testnet => {
                let blk = Block {
                    block_header: BlockHeader {
                        version: 1,
                        previous_header_hash: [0; 32].into(), // genesis_block has all-0 previous_header_hash
                        time: 4,
                        bits: 5.into(),
                        pubkey: ecvrf::VrfPk::from_bytes(&[6; 32]).unwrap(),
                        iterations: 100000,
                        randomness: rug::Integer::from(8),
                    },
                    proof: vec![],
                };
                IndexedBlock::from_raw(blk)
            }
            Network::Regtest | Network::Unitest => {
                let blk = Block {
                    block_header: BlockHeader {
                        version: 1,
                        previous_header_hash: [0; 32].into(), // genesis_block has all-0 previous_header_hash
                        time: 4,
                        bits: 5.into(),
                        pubkey: ecvrf::VrfPk::from_bytes(&[6; 32]).unwrap(),
                        iterations: 100000,
                        randomness: rug::Integer::from(8),
                    },
                    proof: vec![],
                };
                IndexedBlock::from_raw(blk)
            }
        }
    }

    pub fn default_verification_edge(&self) -> H256 {
        match *self {
            Network::Mainnet => H256::from_reversed_str(
                "0000000000000000030abc968e1bd635736e880b946085c93152969b9a81a6e2",
            ),
            Network::Testnet => H256::from_reversed_str(
                "000000000871ee6842d3648317ccc8a435eb8cc3c2429aee94faff9ba26b05a0",
            ),
            _ => *self.genesis_block().hash(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Network, MAGIC_MAINNET, MAGIC_REGTEST, MAGIC_TESTNET, MAGIC_UNITEST, MAX_BITS_MAINNET,
        MAX_BITS_REGTEST, MAX_BITS_TESTNET,
    };
    use compact::Compact;

    #[test]
    fn test_network_magic_number() {
        assert_eq!(MAGIC_MAINNET, Network::Mainnet.magic());
        assert_eq!(MAGIC_TESTNET, Network::Testnet.magic());
        assert_eq!(MAGIC_REGTEST, Network::Regtest.magic());
        assert_eq!(MAGIC_UNITEST, Network::Unitest.magic());
    }

    #[test]
    fn test_network_max_bits() {
        assert_eq!(Network::Mainnet.max_bits(), *MAX_BITS_MAINNET);
        assert_eq!(Network::Testnet.max_bits(), *MAX_BITS_TESTNET);
        assert_eq!(Network::Regtest.max_bits(), *MAX_BITS_REGTEST);
        assert_eq!(Network::Unitest.max_bits(), Compact::max_value().into());
    }

    #[test]
    fn test_network_port() {
        assert_eq!(Network::Mainnet.port(), 8333);
        assert_eq!(Network::Testnet.port(), 18333);
        assert_eq!(Network::Regtest.port(), 18444);
        assert_eq!(Network::Unitest.port(), 18444);
    }

    #[test]
    fn test_network_rpc_port() {
        assert_eq!(Network::Mainnet.rpc_port(), 8332);
        assert_eq!(Network::Testnet.rpc_port(), 18332);
        assert_eq!(Network::Regtest.rpc_port(), 18443);
        assert_eq!(Network::Unitest.rpc_port(), 18443);
    }
}
