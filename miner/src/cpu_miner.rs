use std::time::{Duration, Instant};

use block_assembler::BlockTemplate;
use chain::BlockHeader;
use crypto::dhash256;
use ecvrf::VrfPk;
use primitives::bytes::Bytes;
use rug::{integer::Order, Integer};
use ser::{serialize, Stream};
use sha2::{Digest, Sha256};
use verification::is_valid_proof_of_work_hash;

const STEP: u32 = 1024;

// consistent with verification/src/verify_block.rs
fn h_g(block: &BlockTemplate, pubkey: &VrfPk) -> Integer {
    let mut stream = Stream::default();
    stream
        .append(&block.version)
        .append(&block.previous_header_hash)
        .append(&block.time)
        .append(&block.bits)
        .append(&Bytes::from(pubkey.to_bytes().to_vec()));
    let data = stream.out();
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

/// Cpu miner solution.
pub struct Solution {
    pub iterations: u32,
    pub randomness: Integer,
    pub proof: vdf::Proof,
}

/// Simple randchain cpu miner.
pub fn find_solution(block: &BlockTemplate, pubkey: &VrfPk, timeout: Duration) -> Option<Solution> {
    let start_time = Instant::now();

    let g = h_g(block, pubkey);
    let mut cur_y = g.clone();
    let mut iterations = 0u64;
    loop {
        if timeout != Duration::new(0, 0) && start_time.elapsed() > timeout {
            return None;
        }

        iterations += STEP as u64;
        if iterations > (u32::max_value() as u64) {
            return None;
        }

        let new_y = vdf::eval(&cur_y, STEP);
        // consistent with chain/src/block_header.rs
        let block_header_hash = dhash256(&serialize(&BlockHeader {
            version: block.version,
            previous_header_hash: block.previous_header_hash,
            time: block.time,
            bits: block.bits,
            pubkey: pubkey.clone(),
            iterations: iterations as u32,
            randomness: new_y.clone(),
        }));
        if is_valid_proof_of_work_hash(block.bits, &block_header_hash) {
            let solution = Solution {
                iterations: iterations as u32,
                randomness: new_y.clone(),
                proof: vdf::prove(&g, &new_y, iterations as u32),
            };

            return Some(solution);
        }

        cur_y = new_y;
    }
}

#[cfg(test)]
mod tests {
    use super::find_solution;
    use block_assembler::BlockTemplate;
    use ecvrf::VrfPk;
    use primitives::bigint::{Uint, U256};
    use std::time::Duration;

    #[test]
    fn test_cpu_miner_low_difficulty() {
        let block_template = BlockTemplate {
            version: 0,
            previous_header_hash: 0.into(),
            time: 0,
            bits: U256::max_value().into(),
            height: 0,
        };

        // generate or load key
        let pubkey: VrfPk = VrfPk::from_bytes(&[0; 32]).unwrap();
        let solution = find_solution(&block_template, &pubkey, Duration::from_secs(0));
        assert!(solution.is_some());
    }
}
