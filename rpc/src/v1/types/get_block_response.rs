use super::block::RawBlock;
use super::hash::H256;
use super::uint::U256;
use serde::{Serialize, Serializer};

/// Response to getblock RPC request
#[derive(Debug)]
pub enum GetBlockResponse {
    /// When asking for short response
    Raw(RawBlock),
    /// When asking for verbose response
    Verbose(VerboseBlock),
}

/// Verbose block information
#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VerboseBlock {
    /// Block hash
    pub hash: H256,
    /// Number of confirmations. -1 if block is on the side chain
    pub confirmations: i64,
    /// Block size
    pub size: u32,
    /// Block height
    /// TODO: bitcoind always returns value, but we hold this value for main chain blocks only
    pub height: Option<u32>,
    /// Block version
    pub version: u32,
    /// Block version as hex
    #[serde(rename = "versionHex")]
    pub version_hex: String,
    /// Pubkey as hex
    #[serde(rename = "pubkeyHex")]
    pub pubkey_hex: String,
    /// Randomness as hex
    #[serde(rename = "randomnessHex")]
    pub randomness_hex: String,
    /// Block time in seconds since epoch (Jan 1 1970 GMT)
    pub time: u32,
    // TODO: what is mediantime? remove mediantime?
    /// Median block time in seconds since epoch (Jan 1 1970 GMT)
    /// TODO: bitcoind always returns value, but we can calculate this only if height(block) > 2
    pub mediantime: Option<u32>,
    /// Block iterations
    pub iterations: u32,
    /// Block nbits
    pub bits: u32,
    /// Block difficulty
    pub difficulty: f64,
    /// Expected number of hashes required to produce the chain up to this block (in hex)
    pub chainwork: U256,
    /// Hash of previous block
    pub previousblockhash: Option<H256>,
    /// Hash of next block
    pub nextblockhash: Option<H256>,
}

impl Serialize for GetBlockResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match *self {
            GetBlockResponse::Raw(ref raw_block) => raw_block.serialize(serializer),
            GetBlockResponse::Verbose(ref verbose_block) => verbose_block.serialize(serializer),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::bytes::Bytes;
    use super::super::hash::H256;
    use super::super::uint::U256;
    use super::*;
    use serde_json;

    #[test]
    fn verbose_block_serialize() {
        let block = VerboseBlock::default();
        assert_eq!(
            serde_json::to_string(&block).unwrap(),
            r#"{"hash":"0000000000000000000000000000000000000000000000000000000000000000","confirmations":0,"size":0,"height":null,"version":0,"versionHex":"","pubkeyHex":"","randomnessHex":"","time":0,"mediantime":null,"iterations":0,"bits":0,"difficulty":0.0,"chainwork":"0","previousblockhash":null,"nextblockhash":null}"#
        );

        let block = VerboseBlock {
            hash: H256::from(1),
            confirmations: -1,
            size: 500000,
            height: Some(3513513),
            version: 1,
            version_hex: "01".to_owned(),
            pubkey_hex: "6969696969696969696969696969696969696969696969696969696969696969"
                .to_owned(),
            randomness_hex: "7788".to_owned(),
            time: 111,
            mediantime: Some(100),
            iterations: 124,
            bits: 13513,
            difficulty: 555.555,
            chainwork: U256::from(3),
            previousblockhash: Some(H256::from(4)),
            nextblockhash: Some(H256::from(5)),
        };
        assert_eq!(
            serde_json::to_string(&block).unwrap(),
            r#"{"hash":"0100000000000000000000000000000000000000000000000000000000000000","confirmations":-1,"size":500000,"height":3513513,"version":1,"versionHex":"01","pubkeyHex":"6969696969696969696969696969696969696969696969696969696969696969","randomnessHex":"7788","time":111,"mediantime":100,"iterations":124,"bits":13513,"difficulty":555.555,"chainwork":"3","previousblockhash":"0400000000000000000000000000000000000000000000000000000000000000","nextblockhash":"0500000000000000000000000000000000000000000000000000000000000000"}"#
        );
    }

    #[test]
    fn verbose_block_deserialize() {
        let block = VerboseBlock::default();
        assert_eq!(
			serde_json::from_str::<VerboseBlock>(r#"{"hash":"0000000000000000000000000000000000000000000000000000000000000000","confirmations":0,"size":0,"strippedsize":0,"weight":0,"height":null,"version":0,"versionHex":"","pubkeyHex":"","randomnessHex":"","time":0,"mediantime":null,"iterations":0,"bits":0,"difficulty":0.0,"chainwork":"0","previousblockhash":null,"nextblockhash":null}"#).unwrap(),
			block);

        let block = VerboseBlock {
            hash: H256::from(1),
            confirmations: -1,
            size: 500000,
            height: Some(3513513),
            version: 1,
            version_hex: "01".to_owned(),
            pubkey_hex: "6969696969696969696969696969696969696969696969696969696969696969"
                .to_owned(),
            randomness_hex: "7788".to_owned(),
            time: 111,
            mediantime: Some(100),
            iterations: 124,
            bits: 13513,
            difficulty: 555.555,
            chainwork: U256::from(3),
            previousblockhash: Some(H256::from(4)),
            nextblockhash: Some(H256::from(5)),
        };
        assert_eq!(
			serde_json::from_str::<VerboseBlock>(r#"{"hash":"0100000000000000000000000000000000000000000000000000000000000000","confirmations":-1,"size":500000,"strippedsize":444444,"weight":5236235,"height":3513513,"version":1,"versionHex":"01","pubkeyHex":"6969696969696969696969696969696969696969696969696969696969696969","randomnessHex":"7788","time":111,"mediantime":100,"iterations":124,"bits":13513,"difficulty":555.555,"chainwork":"3","previousblockhash":"0400000000000000000000000000000000000000000000000000000000000000","nextblockhash":"0500000000000000000000000000000000000000000000000000000000000000"}"#).unwrap(),
			block);
    }

    #[test]
    fn get_block_response_raw_serialize() {
        let raw_response = GetBlockResponse::Raw(Bytes::new(vec![0]));
        assert_eq!(serde_json::to_string(&raw_response).unwrap(), r#""00""#);
    }

    #[test]
    fn get_block_response_verbose_serialize() {
        let block = VerboseBlock::default();
        let verbose_response = GetBlockResponse::Verbose(block);
        assert_eq!(
            serde_json::to_string(&verbose_response).unwrap(),
            r#"{"hash":"0000000000000000000000000000000000000000000000000000000000000000","confirmations":0,"size":0,"height":null,"version":0,"versionHex":"","pubkeyHex":"","randomnessHex":"","time":0,"mediantime":null,"iterations":0,"bits":0,"difficulty":0.0,"chainwork":"0","previousblockhash":null,"nextblockhash":null}"#
        );
    }
}
