use chain::IndexedBlock;
use crypto::dhash256;
use error::Error;
use primitives::bytes::Bytes;
use rug::{integer::Order, Integer};
use ser::Stream;
use sha2::{Digest, Sha256};

pub fn h_g(block: &IndexedBlock) -> Integer {
    let mut stream = Stream::default();
    stream
        .append(&block.header.raw.version)
        .append(&block.header.raw.previous_header_hash)
        .append(&block.header.raw.time)
        .append(&block.header.raw.bits)
        .append(&Bytes::from(block.header.raw.pubkey.to_bytes().to_vec()));
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

pub struct BlockVerifier<'a> {
    pub vdf: BlockVDF<'a>,
}

impl<'a> BlockVerifier<'a> {
    pub fn new(block: &'a IndexedBlock) -> Self {
        BlockVerifier {
            vdf: BlockVDF::new(block),
        }
    }

    pub fn check(&self) -> Result<(), Error> {
        self.vdf.check()
    }
}

pub struct BlockVDF<'a> {
    block: &'a IndexedBlock,
}

impl<'a> BlockVDF<'a> {
    fn new(block: &'a IndexedBlock) -> Self {
        BlockVDF { block: block }
    }

    fn check(&self) -> Result<(), Error> {
        let g = h_g(self.block);

        match vdf::verify(
            &g,
            &self.block.header.raw.randomness,
            self.block.header.raw.iterations,
            &self.block.proof,
        ) {
            false => Err(Error::Vdf),
            true => Ok(()),
        }
    }
}
