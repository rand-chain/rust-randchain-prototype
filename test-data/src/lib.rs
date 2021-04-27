//! Various chain-specific test dummies

extern crate ecvrf;
extern crate rug;
extern crate time;

extern crate chain;
extern crate network;
extern crate primitives;
extern crate serialization as ser;
extern crate vdf;
extern crate verification;

use ecvrf::VrfPk;

use chain::Block;
use network::Network::Mainnet;
use primitives::compact::Compact;

pub mod block;
pub mod chain_builder;
pub mod invoke;

pub use block::{
    block_builder, block_hash_builder, build_n_empty_blocks, build_n_empty_blocks_from,
    build_n_empty_blocks_from_genesis,
};
pub use chain_builder::ChainBuilder;

pub fn genesis() -> Block {
    block_h0()
}

pub fn block_h0() -> Block {
    block::block_builder()
        .header()
        .parent(0.into())
        .time(1000)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(4)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h1() -> Block {
    block::block_builder()
        .header()
        .parent(block_h0().hash())
        .time(1001)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(4)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h2() -> Block {
    block::block_builder()
        .header()
        .parent(block_h1().hash())
        .time(1002)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(4)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h3() -> Block {
    block::block_builder()
        .header()
        .parent(block_h2().hash())
        .time(1003)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(1)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h169() -> Block {
    block::block_builder()
        .header()
        .parent("6868686868686868686868686868686868686868686868686868686868686868".into())
        .time(1169)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(1)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h170() -> Block {
    block::block_builder()
        .header()
        .parent(block_h169().hash())
        .time(1170)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(1)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h181() -> Block {
    block::block_builder()
        .header()
        .parent("8080808080808080808080808080808080808080808080808080808080808080".into())
        .time(1181)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(4)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h182() -> Block {
    block::block_builder()
        .header()
        .parent(block_h181().hash())
        .time(1182)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(4)
        .evaluated()
        .build()
        .proved()
        .build()
}

pub fn block_h221() -> Block {
    block::block_builder()
        .header()
        .parent("2020202020202020202020202020202020202020202020202020202020202020".into())
        .time(1221)
        .bits(Compact::from_u256(Mainnet.max_bits()))
        .version(1)
        .pubkey(VrfPk::from_bytes(&[0; 32]).unwrap())
        .iterations(4)
        .evaluated()
        .build()
        .proved()
        .build()
}
