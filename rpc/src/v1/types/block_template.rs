use super::hash::H256;
use miner;
use std::collections::HashMap;

/// Block template
#[derive(Debug, Default, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct BlockTemplate {
    /// The preferred block version
    pub version: u32,
    /// Specific block rules that are to be enforced
    pub rules: Option<Vec<String>>,
    /// Set of pending, supported versionbit (BIP 9) softfork deployments
    /// Keys: named softfork rules
    /// Values: identifies the bit number as indicating acceptance and readiness for given key
    pub vbavailable: Option<HashMap<String, u32>>,
    /// Bit mask of versionbits the server requires set in submissions
    pub vbrequired: Option<u32>,
    /// The hash of previous (best known) block
    pub previousblockhash: H256,
    // TODO:
    /// Data that should be included in the coinbase's scriptSig content
    /// Keys: ignored
    /// Values: value to be included in scriptSig
    pub coinbaseaux: Option<HashMap<String, String>>,
    /// The hash target
    pub target: H256,
    /// The minimum timestamp appropriate for next block time in seconds since epoch (Jan 1 1970 GMT)
    pub mintime: Option<i64>,
    /// List of ways the block template may be changed, e.g. 'time', 'transactions', 'prevblock'
    pub mutable: Option<Vec<String>>,
    /// Limit of block weight
    pub weightlimit: Option<u32>,
    /// Current timestamp in seconds since epoch (Jan 1 1970 GMT)
    pub curtime: u32,
    /// Compressed target of next block
    pub bits: u32,
    /// The height of the next block
    pub height: u32,
}

impl From<miner::BlockTemplate> for BlockTemplate {
    fn from(block: miner::BlockTemplate) -> Self {
        BlockTemplate {
            version: block.version,
            previousblockhash: block.previous_header_hash.reversed().into(),
            curtime: block.time,
            bits: block.bits.into(),
            height: block.height,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::hash::H256;
    use super::*;
    use serde_json;

    #[test]
    fn block_template_serialize() {
        assert_eq!(
            serde_json::to_string(&BlockTemplate {
                version: 0,
                rules: None,
                vbavailable: None,
                vbrequired: None,
                previousblockhash: H256::default(),
                coinbaseaux: None,
                target: H256::default(),
                mintime: None,
                mutable: None,
                weightlimit: None,
                curtime: 100,
                bits: 200,
                height: 300,
            })
            .unwrap(),
            r#"{"version":0,"rules":null,"vbavailable":null,"vbrequired":null,"previousblockhash":"0000000000000000000000000000000000000000000000000000000000000000","coinbaseaux":null,"target":"0000000000000000000000000000000000000000000000000000000000000000","mintime":null,"mutable":null,"weightlimit":null,"curtime":100,"bits":200,"height":300}"#
        );
        assert_eq!(
            serde_json::to_string(&BlockTemplate {
                version: 0,
                rules: Some(vec!["a".to_owned()]),
                vbavailable: Some(vec![("b".to_owned(), 5)].into_iter().collect()),
                vbrequired: Some(10),
                previousblockhash: H256::from(10),
                coinbaseaux: Some(vec![("c".to_owned(), "d".to_owned())].into_iter().collect()),
                target: H256::from(100),
                mintime: Some(7),
                mutable: Some(vec!["afg".to_owned()]),
                weightlimit: Some(523),
                curtime: 100,
                bits: 200,
                height: 300,
            })
            .unwrap(),
            r#"{"version":0,"rules":["a"],"vbavailable":{"b":5},"vbrequired":10,"previousblockhash":"0a00000000000000000000000000000000000000000000000000000000000000","coinbaseaux":{"c":"d"},"target":"6400000000000000000000000000000000000000000000000000000000000000","mintime":7,"mutable":["afg"],"weightlimit":523,"curtime":100,"bits":200,"height":300}"#
        );
    }

    #[test]
    fn block_template_deserialize() {
        assert_eq!(
			serde_json::from_str::<BlockTemplate>(r#"{"version":0,"rules":null,"vbavailable":null,"vbrequired":null,"previousblockhash":"0000000000000000000000000000000000000000000000000000000000000000","transactions":[],"coinbaseaux":null,"coinbasevalue":null,"coinbasetxn":null,"target":"0000000000000000000000000000000000000000000000000000000000000000","mintime":null,"mutable":null,"noncerange":null,"sigoplimit":null,"sizelimit":null,"weightlimit":null,"curtime":100,"bits":200,"height":300}"#).unwrap(),
			BlockTemplate {
				version: 0,
				rules: None,
				vbavailable: None,
				vbrequired: None,
				previousblockhash: H256::default(),
				coinbaseaux: None,
				target: H256::default(),
				mintime: None,
				mutable: None,
				weightlimit: None,
				curtime: 100,
				bits: 200,
				height: 300,
			});
        assert_eq!(
			serde_json::from_str::<BlockTemplate>(r#"{"version":0,"rules":["a"],"vbavailable":{"b":5},"vbrequired":10,"previousblockhash":"0a00000000000000000000000000000000000000000000000000000000000000","transactions":[{"data":"00010203","txid":null,"hash":null,"depends":null,"fee":null,"sigops":null,"weight":null,"required":false}],"coinbaseaux":{"c":"d"},"coinbasevalue":30,"coinbasetxn":{"data":"555555","txid":"2c00000000000000000000000000000000000000000000000000000000000000","hash":"3700000000000000000000000000000000000000000000000000000000000000","depends":[1],"fee":300,"sigops":400,"weight":500,"required":true},"target":"6400000000000000000000000000000000000000000000000000000000000000","mintime":7,"mutable":["afg"],"noncerange":"00000000ffffffff","sigoplimit":45,"sizelimit":449,"weightlimit":523,"curtime":100,"bits":200,"height":300}"#).unwrap(),
			BlockTemplate {
				version: 0,
				rules: Some(vec!["a".to_owned()]),
				vbavailable: Some(vec![("b".to_owned(), 5)].into_iter().collect()),
				vbrequired: Some(10),
				previousblockhash: H256::from(10),
				coinbaseaux: Some(vec![("c".to_owned(), "d".to_owned())].into_iter().collect()),
				target: H256::from(100),
				mintime: Some(7),
				mutable: Some(vec!["afg".to_owned()]),
				weightlimit: Some(523),
				curtime: 100,
				bits: 200,
				height: 300,
			});
    }
}
