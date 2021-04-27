use chain::BlockHeader;
use ser::{CompactInteger, Deserializable, Error as ReaderError, Reader, Serializable, Stream};
use std::io;
use {MessageResult, Payload};

pub const HEADERS_MAX_HEADERS_LEN: usize = 2000;

#[derive(Debug, PartialEq)]
pub struct Headers {
    pub headers: Vec<BlockHeader>,
}

impl Headers {
    pub fn with_headers(headers: Vec<BlockHeader>) -> Self {
        Headers { headers: headers }
    }
}

#[derive(Debug, PartialEq)]
struct Header {
    header: BlockHeader,
}

impl From<Header> for BlockHeader {
    fn from(header: Header) -> BlockHeader {
        header.header
    }
}

#[derive(Debug, PartialEq)]
struct HeaderRef<'a> {
    header: &'a BlockHeader,
}

impl<'a> From<&'a BlockHeader> for HeaderRef<'a> {
    fn from(header: &'a BlockHeader) -> Self {
        HeaderRef { header: header }
    }
}

impl Payload for Headers {
    fn version() -> u32 {
        0
    }

    fn command() -> &'static str {
        "headers"
    }

    fn deserialize_payload<T>(reader: &mut Reader<T>, _version: u32) -> MessageResult<Self>
    where
        T: io::Read,
    {
        let header_vec: Vec<Header> = reader.read_list()?;
        let headers = Headers {
            headers: header_vec.into_iter().map(Into::into).collect(),
        };

        Ok(headers)
    }

    fn serialize_payload(&self, stream: &mut Stream, _version: u32) -> MessageResult<()> {
        let header_vec: Vec<HeaderRef> = self.headers.iter().map(Into::into).collect();
        stream.append_list(&header_vec);
        Ok(())
    }
}

impl<'a> Serializable for HeaderRef<'a> {
    fn serialize(&self, stream: &mut Stream) {
        stream
            .append(self.header)
            .append(&CompactInteger::from(0u32));
    }
}

impl Deserializable for Header {
    fn deserialize<T>(reader: &mut Reader<T>) -> Result<Self, ReaderError>
    where
        T: io::Read,
    {
        let header = Header {
            header: reader.read()?,
        };

        let txn_count: CompactInteger = reader.read()?;
        if txn_count != 0u32.into() {
            return Err(ReaderError::MalformedData);
        }

        Ok(header)
    }
}
