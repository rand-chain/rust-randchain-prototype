use block::Block;
use hash::H256;
use hex::FromHex;
use indexed_header::IndexedBlockHeader;
use rug::Integer;
use ser::{deserialize, serialized_list_size};
use ser::{Deserializable, Error as ReaderError, Reader, Serializable};
use std::cmp;
use std::io;

#[derive(Debug, Clone)]
pub struct IndexedBlock {
    pub header: IndexedBlockHeader,
    pub proof: vdf::Proof,
}

impl Deserializable for IndexedBlock {
    fn deserialize<T>(reader: &mut Reader<T>) -> Result<Self, ReaderError>
    where
        T: io::Read,
    {
        let res = IndexedBlock {
            header: reader.read()?,
            proof: reader.read_list()?,
        };

        Ok(res)
    }
}

#[cfg(feature = "test-helpers")]
impl From<Block> for IndexedBlock {
    fn from(block: Block) -> Self {
        Self::from_raw(block)
    }
}

impl cmp::PartialEq for IndexedBlock {
    fn eq(&self, other: &Self) -> bool {
        self.header.hash == other.header.hash
    }
}

impl IndexedBlock {
    pub fn new(header: IndexedBlockHeader, proof: vdf::Proof) -> Self {
        IndexedBlock {
            header: header,
            proof: proof,
        }
    }

    /// Explicit conversion of the raw Block into IndexedBlock.
    ///
    /// Hashes block header + transactions.
    pub fn from_raw(block: Block) -> Self {
        let Block {
            block_header,
            proof,
        } = block;
        Self::new(IndexedBlockHeader::from_raw(block_header), proof)
    }

    pub fn hash(&self) -> &H256 {
        &self.header.hash
    }

    pub fn to_raw_block(self) -> Block {
        Block::new(self.header.raw, self.proof)
    }

    pub fn size(&self) -> usize {
        let header_size = self.header.raw.serialized_size();
        let proof_size = serialized_list_size(&self.proof);
        header_size + proof_size
    }

    pub fn randomness(&self) -> &Integer {
        &self.header.raw.randomness
    }
}

impl From<&'static str> for IndexedBlock {
    fn from(s: &'static str) -> Self {
        deserialize(&s.from_hex::<Vec<u8>>().unwrap() as &[u8]).unwrap()
    }
}
