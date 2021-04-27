use bytes::Bytes;
use compact::Compact;
use crypto::dhash256;
use hash::H256;
use hex::FromHex;
use rug::Integer;
use ser::{deserialize, serialize};
use ser::{Deserializable, Error as ReaderError, Reader, Serializable, Stream};
use std::fmt;
use std::io;
use VrfPk;

#[derive(PartialEq, Clone)]
pub struct BlockHeader {
    pub version: u32,
    pub previous_header_hash: H256,
    pub time: u32,
    pub bits: Compact,
    pub pubkey: VrfPk,
    pub iterations: u32,
    pub randomness: Integer,
}

impl BlockHeader {
    /// Compute hash of the block header.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn hash(&self) -> H256 {
        block_header_hash(self)
    }
}

impl Serializable for BlockHeader {
    fn serialize(&self, stream: &mut Stream) {
        stream
            .append(&self.version)
            .append(&self.previous_header_hash)
            .append(&self.time)
            .append(&self.bits)
            .append(&Bytes::from(self.pubkey.to_bytes().to_vec()))
            .append(&self.iterations)
            .append(&self.randomness);
    }
}

impl Deserializable for BlockHeader {
    fn deserialize<T>(reader: &mut Reader<T>) -> Result<Self, ReaderError>
    where
        T: io::Read,
    {
        let res = BlockHeader {
            version: reader.read()?,
            previous_header_hash: reader.read()?,
            time: reader.read()?,
            bits: reader.read()?,
            pubkey: {
                let pk_bytes = reader.read::<Bytes>()?;
                if pk_bytes.len() != 32 {
                    return Err(ReaderError::MalformedData);
                }
                let mut temp: [u8; 32] = [0; 32];
                temp.copy_from_slice(pk_bytes.as_ref());
                match VrfPk::from_bytes(&temp) {
                    Err(_) => return Err(ReaderError::MalformedData),
                    Ok(pk) => pk,
                }
            },
            iterations: reader.read()?,
            randomness: reader.read()?,
        };

        Ok(res)
    }
}

impl fmt::Debug for BlockHeader {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BlockHeader")
            .field("version", &self.version)
            .field(
                "previous_header_hash",
                &self.previous_header_hash.reversed(),
            )
            .field("time", &self.time)
            .field("bits", &self.bits)
            .field("pubkey", &self.pubkey)
            .field("iterations", &self.iterations)
            .field("randomness", &self.randomness)
            .finish()
    }
}

impl From<&'static str> for BlockHeader {
    fn from(s: &'static str) -> Self {
        deserialize(&s.from_hex::<Vec<u8>>().unwrap() as &[u8]).unwrap()
    }
}

/// Compute hash of the block header.
pub(crate) fn block_header_hash(block_header: &BlockHeader) -> H256 {
    dhash256(&serialize(block_header))
}

#[cfg(test)]
mod tests {
    use super::BlockHeader;
    use rug::Integer;
    use ser::{Error as ReaderError, Reader, Stream};
    use VrfPk;

    #[test]
    fn test_block_header_stream() {
        let block_header = BlockHeader {
            version: 1,
            previous_header_hash: [2; 32].into(),
            time: 4,
            bits: 5.into(),
            pubkey: VrfPk::from_bytes(&[6; 32]).unwrap(),
            iterations: 7,
            randomness: Integer::from(8),
        };

        let mut stream = Stream::default();
        stream.append(&block_header);

        let expected = vec![
            0x01, 0x00, 0x00, 0x00, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
            0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
            0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x04, 0x00, 0x00, 0x00, 0x05, 0x00,
            0x00, 0x00, 0x20, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
            0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
            0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x07, 0x00, 0x00, 0x00, 0x01, 0x08,
        ]
        .into();

        assert_eq!(stream.out(), expected);
    }

    #[test]
    fn test_block_header_reader() {
        let buffer = vec![
            0x01, 0x00, 0x00, 0x00, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
            0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02,
            0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x02, 0x04, 0x00, 0x00, 0x00, 0x05, 0x00,
            0x00, 0x00, 0x20, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
            0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06,
            0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x06, 0x07, 0x00, 0x00, 0x00, 0x01, 0x08,
        ];

        let mut reader = Reader::new(&buffer);

        let expected = BlockHeader {
            version: 1,
            previous_header_hash: [2; 32].into(),
            time: 4,
            bits: 5.into(),
            pubkey: VrfPk::from_bytes(&[6; 32]).unwrap(),
            iterations: 7,
            randomness: Integer::from(8),
        };

        assert_eq!(expected, reader.read().unwrap());
        assert_eq!(
            ReaderError::UnexpectedEnd,
            reader.read::<BlockHeader>().unwrap_err()
        );
    }
}
