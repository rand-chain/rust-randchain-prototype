use std::{thread, time};

use ecvrf::VrfPk;
use rand::prelude::*;
use rug::{integer::Order, Integer};
use sha2::{Digest, Sha256};

use block_assembler::BlockTemplate;
use cpu_miner::Solution;
use crypto::dhash256;
use primitives::bytes::Bytes;
use ser::Stream;

const STEP: u32 = 233868;

// consistent with verification/src/verify_block.rs
fn h_g(block: &BlockTemplate, pubkey: &VrfPk) -> Integer {
    let mut stream = Stream::default();
    stream
        .append(&block.version)
        .append(&block.previous_header_hash)
        .append(&block.time)
        .append(&block.bits)
        .append(&Bytes::from(pubkey.to_bytes().to_vec()));

    h_g_inner(&stream.out(), pubkey)
}

// consistent with verification/src/verify_block.rs
fn h_g_inner(data: &Bytes, _pubkey: &VrfPk) -> Integer {
    let seed = dhash256(&data);
    let prefix = "residue_part_".as_bytes();
    // concat 8 sha256 to a 2048-bit hash
    let all_2048: Vec<u8> = (0..((2048 / 256) as u8))
        .map(|index| {
            let mut hasher = Sha256::new();
            hasher.update(prefix);
            hasher.update(vec![index]);
            hasher.update(<[u8; 32]>::from(seed));
            hasher.finalize()
        })
        .flatten()
        .collect();
    let result = Integer::from_digits(&all_2048, Order::Lsf);
    result.div_rem_floor(vdf::MODULUS.clone()).1
}

/// Simple mocking randchain cpu miner.
pub fn try_solve_one_shot(
    block: &BlockTemplate,
    pubkey: &VrfPk,
    mut iterations: u64,
    network_target: u32,
) -> Option<Solution> {
    thread::sleep(time::Duration::from_secs(1));
    let g = h_g(block, pubkey);
    iterations += STEP as u64;
    if iterations > (u32::max_value() as u64) {
        return None;
    }

    let mut rng = rand::thread_rng();
    let r: f32 = rng.gen(); // generates a float between 0 and 1
    if r <= (1f32) / (network_target as f32) {
        let y = h_g_inner(&Bytes::from(r.to_ne_bytes().to_vec()), pubkey);
        let solution = Solution {
            iterations: iterations as u32,
            randomness: y.clone(),
            proof: vdf::prove(&g, &y, iterations as u32),
        };

        return Some(solution);
    }
    return None;
}
