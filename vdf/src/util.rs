use rug::{integer::Order, Integer};
use sha2::{Digest, Sha256};

use super::config::MODULUS;

/// Fiat–Shamir heuristic non-iterative signature
pub fn hash_fs(inputs: &[&Integer]) -> Integer {
    let mut hasher = Sha256::new();
    for input in inputs {
        hasher.update(input.to_digits::<u8>(Order::Lsf));
        hasher.update("\n".as_bytes());
    }
    let seed = hasher.finalize();
    let prefix = "fs_part_".as_bytes();
    // concat 8 sha256 to a 2048-bit hash
    let all_2048: Vec<u8> = (0..((2048 / 256) as u8))
        .map(|index| {
            let mut hasher = Sha256::new();
            hasher.update(prefix);
            hasher.update(vec![index]);
            hasher.update(seed.clone());
            hasher.finalize()
        })
        .flatten()
        .collect();
    let result = Integer::from_digits(&all_2048, Order::Lsf);
    result.div_rem_floor(MODULUS.clone()).1
}
