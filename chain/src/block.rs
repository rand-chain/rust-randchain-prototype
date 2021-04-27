use hex::FromHex;
use ser::deserialize;
use ser::{Deserializable, Error as ReaderError, Reader, Serializable, Stream};
use std::io;
use BlockHeader;

#[cfg(any(test, feature = "test-helpers"))]
use hash::H256;

#[derive(Debug, PartialEq, Clone)]
pub struct Block {
    pub block_header: BlockHeader,
    pub proof: vdf::Proof,
}

impl Serializable for Block {
    fn serialize(&self, stream: &mut Stream) {
        stream.append(&self.block_header).append_list(&self.proof);
    }
}

impl Deserializable for Block {
    fn deserialize<T>(reader: &mut Reader<T>) -> Result<Self, ReaderError>
    where
        T: io::Read,
    {
        let res = Block {
            block_header: reader.read()?,
            proof: reader.read_list()?,
        };

        Ok(res)
    }
}

impl From<&'static str> for Block {
    fn from(s: &'static str) -> Self {
        deserialize(&s.from_hex::<Vec<u8>>().unwrap() as &[u8]).unwrap()
    }
}

impl Block {
    pub fn new(header: BlockHeader, proof: vdf::Proof) -> Self {
        Block {
            block_header: header,
            proof: proof,
        }
    }

    pub fn header(&self) -> &BlockHeader {
        &self.block_header
    }

    #[cfg(any(test, feature = "test-helpers"))]
    pub fn hash(&self) -> H256 {
        self.block_header.hash()
    }
}
