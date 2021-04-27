use hex::ToHex;
use jsonrpc_core::Error;
use jsonrpc_macros::Trailing;
use primitives::hash::H256 as GlobalH256;
use ser::serialize;
use storage;
use v1::helpers::errors::{block_at_height_not_found, block_not_found};
use v1::traits::BlockChain;
use v1::types::H256;
use v1::types::U256;
use v1::types::{GetBlockResponse, RawBlock, VerboseBlock};
use verification;

pub struct BlockChainClient<T: BlockChainClientCoreApi> {
    core: T,
}

pub trait BlockChainClientCoreApi: Send + Sync + 'static {
    fn best_block_hash(&self) -> GlobalH256;
    fn block_count(&self) -> u32;
    fn block_hash(&self, height: u32) -> Option<GlobalH256>;
    fn difficulty(&self) -> f64;
    fn raw_block(&self, hash: GlobalH256) -> Option<RawBlock>;
    fn verbose_block(&self, hash: GlobalH256) -> Option<VerboseBlock>;
}

pub struct BlockChainClientCore {
    storage: storage::SharedStore,
}

impl BlockChainClientCore {
    pub fn new(storage: storage::SharedStore) -> Self {
        BlockChainClientCore { storage: storage }
    }
}

impl BlockChainClientCoreApi for BlockChainClientCore {
    fn best_block_hash(&self) -> GlobalH256 {
        self.storage.best_block().hash
    }

    fn block_count(&self) -> u32 {
        self.storage.best_block().number
    }

    fn block_hash(&self, height: u32) -> Option<GlobalH256> {
        self.storage.block_hash(height)
    }

    fn difficulty(&self) -> f64 {
        self.storage.difficulty()
    }

    fn raw_block(&self, hash: GlobalH256) -> Option<RawBlock> {
        self.storage
            .block(hash.into())
            .map(|block| serialize(&block.to_raw_block()).into())
    }

    fn verbose_block(&self, hash: GlobalH256) -> Option<VerboseBlock> {
        self.storage.block(hash.into()).map(|block| {
            let height = self.storage.block_number(block.hash());
            let confirmations = match height {
                Some(block_number) => (self.storage.best_block().number - block_number + 1) as i64,
                None => -1,
            };
            let block_size = block.size();
            let median_time = verification::median_timestamp(
                &block.header.raw,
                self.storage.as_block_header_provider(),
            );

            VerboseBlock {
                confirmations: confirmations,
                size: block_size as u32,
                height: height,
                mediantime: Some(median_time),
                difficulty: block.header.raw.bits.to_f64(),
                chainwork: U256::default(), // TODO: read from storage
                previousblockhash: Some(block.header.raw.previous_header_hash.clone().into()),
                nextblockhash: height
                    .and_then(|h| self.storage.block_hash(h + 1).map(|h| h.into())),
                bits: block.header.raw.bits.into(),
                hash: block.hash().clone().into(),
                pubkey_hex: block.header.raw.pubkey.to_bytes().to_hex(),
                randomness_hex: block.header.raw.randomness.to_string_radix(16),
                iterations: block.header.raw.iterations,
                time: block.header.raw.time,
                version: block.header.raw.version,
                version_hex: format!("{:x}", &block.header.raw.version),
            }
        })
    }
}

impl<T> BlockChainClient<T>
where
    T: BlockChainClientCoreApi,
{
    pub fn new(core: T) -> Self {
        BlockChainClient { core: core }
    }
}

impl<T> BlockChain for BlockChainClient<T>
where
    T: BlockChainClientCoreApi,
{
    fn best_block_hash(&self) -> Result<H256, Error> {
        Ok(self.core.best_block_hash().reversed().into())
    }

    fn block_count(&self) -> Result<u32, Error> {
        Ok(self.core.block_count())
    }

    fn block_hash(&self, height: u32) -> Result<H256, Error> {
        self.core
            .block_hash(height)
            .map(|h| h.reversed().into())
            .ok_or(block_at_height_not_found(height))
    }

    fn difficulty(&self) -> Result<f64, Error> {
        Ok(self.core.difficulty())
    }

    fn block(&self, hash: H256, verbose: Trailing<bool>) -> Result<GetBlockResponse, Error> {
        let global_hash: GlobalH256 = hash.clone().into();
        if verbose.unwrap_or_default() {
            let verbose_block = self.core.verbose_block(global_hash.reversed());
            if let Some(mut verbose_block) = verbose_block {
                verbose_block.previousblockhash =
                    verbose_block.previousblockhash.map(|h| h.reversed());
                verbose_block.nextblockhash = verbose_block.nextblockhash.map(|h| h.reversed());
                verbose_block.hash = verbose_block.hash.reversed();
                verbose_block.randomness_hex = verbose_block.randomness_hex;
                Some(GetBlockResponse::Verbose(verbose_block))
            } else {
                None
            }
        } else {
            self.core
                .raw_block(global_hash.reversed())
                .map(|block| GetBlockResponse::Raw(block))
        }
        .ok_or(block_not_found(hash))
    }
}

#[cfg(test)]
pub mod tests {
    extern crate test_data;

    use super::*;
    use db::BlockChainDatabase;
    use jsonrpc_core::IoHandler;
    use primitives::hash::H256 as GlobalH256;
    use std::sync::Arc;
    use v1::traits::BlockChain;
    use v1::types::{RawBlock, VerboseBlock};

    #[derive(Default)]
    struct SuccessBlockChainClientCore;
    #[derive(Default)]
    struct ErrorBlockChainClientCore;

    impl BlockChainClientCoreApi for SuccessBlockChainClientCore {
        fn best_block_hash(&self) -> GlobalH256 {
            test_data::genesis().hash()
        }

        fn block_count(&self) -> u32 {
            1
        }

        fn block_hash(&self, _height: u32) -> Option<GlobalH256> {
            Some(test_data::genesis().hash())
        }

        fn difficulty(&self) -> f64 {
            1f64
        }

        fn raw_block(&self, _hash: GlobalH256) -> Option<RawBlock> {
            let b2_bytes = serialize(&test_data::block_h2());
            Some(RawBlock::from(b2_bytes))
        }

        fn verbose_block(&self, _hash: GlobalH256) -> Option<VerboseBlock> {
            Some(VerboseBlock {
                hash: test_data::block_h2().hash().into(),
                confirmations: 1, // h2
                size: serialize(&test_data::block_h2()).len() as u32,
                height: Some(2),
                version: 1,
                version_hex: "1".to_owned(),
                pubkey_hex: test_data::block_h2().header().pubkey.to_bytes().to_hex(),
                randomness_hex: test_data::block_h2()
                    .header()
                    .randomness
                    .to_string_radix(16),
                time: test_data::block_h2().header().time,
                mediantime: None,
                iterations: test_data::block_h2().header().iterations,
                bits: test_data::block_h2().header().bits.into(),
                difficulty: 1.0,
                chainwork: 0.into(),
                previousblockhash: Some(test_data::block_h1().hash().into()),
                nextblockhash: None,
            })
        }
    }

    impl BlockChainClientCoreApi for ErrorBlockChainClientCore {
        fn best_block_hash(&self) -> GlobalH256 {
            test_data::genesis().hash()
        }

        fn block_count(&self) -> u32 {
            1
        }

        fn block_hash(&self, _height: u32) -> Option<GlobalH256> {
            None
        }

        fn difficulty(&self) -> f64 {
            1f64
        }

        fn raw_block(&self, _hash: GlobalH256) -> Option<RawBlock> {
            None
        }

        fn verbose_block(&self, _hash: GlobalH256) -> Option<VerboseBlock> {
            None
        }
    }

    #[test]
    fn best_block_hash_success() {
        let client = BlockChainClient::new(SuccessBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
            .handle_request_sync(
                &(r#"
			{
				"jsonrpc": "2.0",
				"method": "getbestblockhash",
				"params": [],
				"id": 1
			}"#),
            )
            .unwrap();

        // direct hash is 0e02cf1de23136c83e2aa473e226c02f5f2d4a7dffd16e49fafdfb5e7d6f5971
        // but client expects reverse hash
        assert_eq!(
            &sample,
            r#"{"jsonrpc":"2.0","result":"71596f7d5efbfdfa496ed1ff7d4a2d5f2fc026e273a42a3ec83631e21dcf020e","id":1}"#
        );
    }

    #[test]
    fn block_count_success() {
        let client = BlockChainClient::new(SuccessBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
            .handle_request_sync(
                &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblockcount",
                    	"params": [],
                    	"id": 1
                    }"#),
            )
            .unwrap();

        assert_eq!(&sample, r#"{"jsonrpc":"2.0","result":1,"id":1}"#);
    }

    #[test]
    fn block_hash_success() {
        let client = BlockChainClient::new(SuccessBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
            .handle_request_sync(
                &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblockhash",
                    	"params": [0],
                    	"id": 1
                    }"#),
            )
            .unwrap();

        // direct hash is 0e02cf1de23136c83e2aa473e226c02f5f2d4a7dffd16e49fafdfb5e7d6f5971
        // but client expects reverse hash
        assert_eq!(
            &sample,
            r#"{"jsonrpc":"2.0","result":"71596f7d5efbfdfa496ed1ff7d4a2d5f2fc026e273a42a3ec83631e21dcf020e","id":1}"#
        );
    }

    #[test]
    fn block_hash_error() {
        let client = BlockChainClient::new(ErrorBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
            .handle_request_sync(
                &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblockhash",
                    	"params": [0],
                    	"id": 1
                    }"#),
            )
            .unwrap();

        assert_eq!(
            &sample,
            r#"{"jsonrpc":"2.0","error":{"code":-32099,"message":"Block at given height is not found","data":"0"},"id":1}"#
        );
    }

    #[test]
    fn difficulty_success() {
        let client = BlockChainClient::new(SuccessBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
            .handle_request_sync(
                &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getdifficulty",
                    	"params": [],
                    	"id": 1
                    }"#),
            )
            .unwrap();

        assert_eq!(&sample, r#"{"jsonrpc":"2.0","result":1.0,"id":1}"#);
    }

    #[test]
    fn verbose_block_contents() {
        let storage = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
            test_data::block_h1().into(),
            test_data::block_h2().into(),
        ]));

        let core = BlockChainClientCore::new(storage);

        // get info on block #1:
        let verbose_block = core.verbose_block(test_data::block_h1().hash().into());
        assert_eq!(
            verbose_block,
            Some(VerboseBlock {
                hash: test_data::block_h1().hash().into(),
                confirmations: 2, // h1 + h2
                size: 859,
                height: Some(1),
                version: 1,
                version_hex: "1".to_owned(),
                pubkey_hex: "0000000000000000000000000000000000000000000000000000000000000000"
                    .to_owned(),
                randomness_hex: "966e8f70aa11fdb48b7d463c57db4722d1ac994640879f98a616b165c3a5df9db31c0dee6cbcebf46c427049ead7b7edbe39970ce7b0fecf2a9b07c4381e466055f1e8fc152f304f3cfdbb475bdaa93a9f95de38ff1d091d175bfea579574f444c1650a6ab4bfd97d2b19f6653549517d91b882a5560da1b6b0188ae67d016dfe2dfe795593e844b2d32e29b985b425038c83c7aaf9b9ea3d73065767f234da6f25e43137a8d91265d16afdb9ab728f619385a31734ba769bfb5ff1bea3869043534d1697efc3148a0867ceacb6d2d24508ea1b201abfbb23604aa7cc3dd7dbfe9c002f2f26199aea18e9c24e870bfa090505eecc307d895caf7bdadccbdfbb4".to_owned(),
                time: 1001,
                mediantime: Some(1000),
                iterations: 4,
                bits: 545259519,
                difficulty: 0.00000000046565423739069247,
                chainwork: 0.into(),
                previousblockhash: Some(
                    test_data::genesis().hash().into()
                ),
                nextblockhash: Some(
                    test_data::block_h2().hash().into()
                ),
            })
        );

        // get info on block #2:
        let verbose_block = core.verbose_block(test_data::block_h2().hash().into());
        assert_eq!(
            verbose_block,
            Some(VerboseBlock {
                hash: test_data::block_h2().hash().into(),
                confirmations: 1, // h2
                size: 859,
                height: Some(2),
                version: 1,
                version_hex: "1".to_owned(),
                pubkey_hex: "0000000000000000000000000000000000000000000000000000000000000000"
                    .to_owned(),
                randomness_hex: "5ced562ee6e1c95c2e034d3ed4fc38508fd4cf0a1faa466303c79b7bc9fcaf4d60d1dc53884698e891e5ee1f661f79e58631b8003d0fd18d526fcc3f95e97597c125573895ffc3a6a9b1458b1a383cdb1ea151f2b01f62980d6e92aaef8d0c5a06e56995176a75cb8aa37c94a5e677e1dd91e1fb3874b72f614507b5e9b29bc5e43264123e8fb29664558dad1aa4e350843262effaffe63685765fa7028ce10b81d13059a3dfc8207b7bc37ce4e8e23d22a797b454abf7777c641a534a2bfd5d67f5d182c75a4a7fe9f3a31eb85afe533cc55edca86b9dfe6e8d66c2c00a90097ac0ff8ca06f00dd524018fb422ad68994ec537cd12ab10f4144a8f3290dae1a".to_owned(),
                time: 1002,
                mediantime: Some(1001),
                iterations: 4,
                bits: 545259519,
                difficulty: 0.00000000046565423739069247,
                chainwork: 0.into(),
                previousblockhash: Some(
                    test_data::block_h1().hash().into()
                ),
                nextblockhash: None,
            })
        );
    }

    #[test]
    fn raw_block_success() {
        let client = BlockChainClient::new(SuccessBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let expected = r#"{"jsonrpc":"2.0","result":"0100000027cf1cfd394338d0af68c0bd84960c8cb1becb8a10991df558a8fb167ef65e63ea030000ffff7f2020000000000000000000000000000000000000000000000000000000000000000004000000fd00015ced562ee6e1c95c2e034d3ed4fc38508fd4cf0a1faa466303c79b7bc9fcaf4d60d1dc53884698e891e5ee1f661f79e58631b8003d0fd18d526fcc3f95e97597c125573895ffc3a6a9b1458b1a383cdb1ea151f2b01f62980d6e92aaef8d0c5a06e56995176a75cb8aa37c94a5e677e1dd91e1fb3874b72f614507b5e9b29bc5e43264123e8fb29664558dad1aa4e350843262effaffe63685765fa7028ce10b81d13059a3dfc8207b7bc37ce4e8e23d22a797b454abf7777c641a534a2bfd5d67f5d182c75a4a7fe9f3a31eb85afe533cc55edca86b9dfe6e8d66c2c00a90097ac0ff8ca06f00dd524018fb422ad68994ec537cd12ab10f4144a8f3290dae1a02fd000113f6cd3e8a5010bd7383bc1540735721fa5f6b433e67c5dfb9a4d9e30559ae3c08627385820f34535edcad1d1b36019e5f615e70661c3e9cb553643fd8003096a3079c67ac289eb122abb1d7cace1ae8f4a3ec6959b89c43a16ffe785fb166ebe94c2609d65795fc58f6faac12e6e00a41099b44f6a53344db5f2846bf2c5782a503234822f8d75257d5a04d352cc85fb569157e8c1195f77208359759229f1c3e81ae82275b6362350a7fb430abf99c1a0d054eaa4aaf90e63fb1028740af75ff05c8286338aef128f6fb75d93a78c08dd4e05f0b91516cb15b1832d473f5539daf6b6c1176ff460ace9049029278640be9bc5a1893030387d72760e1ff3532fd00014be8ac242b610c307dee148c2d3cbf0e460e6dbf00aadaf1ee202c87a630ec96b74d6caedf9932c98d35949cdfbca69e32ae72b8b200dc41f52ea5a890c3fc64370c124f9fa9478fcaaa8da260ffdb07492ebc60ee642f459fa75a7e9839fb8442f03610a807a662a45364f729352682d02b4a2c353c11d0dde3a802bceb8af7149f499152d1e7b51bc627abaa63e270442cdf0a3d9d12aa2590abd0ab9209faca4a2b3c8cb1ff673e282f3d89fcee7d28b1139b1c0646ad2248a628fbc71cb1228a4ced677e589b17b8f688032921237cf2d723c204a89259d321a8c88cb18303669ef278f6480ea1bad355cc86d431157bff750d4918af83c47f1889efa05e","id":1}"#;

        let sample = handler
             .handle_request_sync(
                 &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblock",
                    	"params": ["29483ce82fad9d817f3ac76b8bd8f221cd5a6aa882523da8fcf19df6c0f60d40", false],
                    	"id": 1
                    }"#),
             )
             .unwrap();
        assert_eq!(&sample, expected);

        // try without optional parameter
        let sample = handler
            .handle_request_sync(
                &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblock",
                    	"params": ["29483ce82fad9d817f3ac76b8bd8f221cd5a6aa882523da8fcf19df6c0f60d40"],
                    	"id": 1
                    }"#),
            )
            .unwrap();
        assert_eq!(&sample, expected);
    }

    #[test]
    fn raw_block_error() {
        let client = BlockChainClient::new(ErrorBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
            .handle_request_sync(
                &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblock",
                    	"params": ["000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd", false],
                    	"id": 1
                    }"#),
            )
            .unwrap();

        assert_eq!(
            &sample,
            r#"{"jsonrpc":"2.0","error":{"code":-32099,"message":"Block with given hash is not found","data":"000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd"},"id":1}"#
        );
    }

    #[test]
    fn verbose_block_success() {
        let client = BlockChainClient::new(SuccessBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
             .handle_request_sync(
                 &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblock",
                    	"params": ["29483ce82fad9d817f3ac76b8bd8f221cd5a6aa882523da8fcf19df6c0f60d40",true],
                    	"id": 1
                    }"#),
             )
             .unwrap();

        assert_eq!(
            &sample,
            r#"{"jsonrpc":"2.0","result":{"bits":545259519,"chainwork":"0","confirmations":1,"difficulty":1.0,"hash":"29483ce82fad9d817f3ac76b8bd8f221cd5a6aa882523da8fcf19df6c0f60d40","height":2,"iterations":4,"mediantime":null,"nextblockhash":null,"previousblockhash":"635ef67e16fba858f51d99108acbbeb18c0c9684bdc068afd0384339fd1ccf27","pubkeyHex":"0000000000000000000000000000000000000000000000000000000000000000","randomnessHex":"5ced562ee6e1c95c2e034d3ed4fc38508fd4cf0a1faa466303c79b7bc9fcaf4d60d1dc53884698e891e5ee1f661f79e58631b8003d0fd18d526fcc3f95e97597c125573895ffc3a6a9b1458b1a383cdb1ea151f2b01f62980d6e92aaef8d0c5a06e56995176a75cb8aa37c94a5e677e1dd91e1fb3874b72f614507b5e9b29bc5e43264123e8fb29664558dad1aa4e350843262effaffe63685765fa7028ce10b81d13059a3dfc8207b7bc37ce4e8e23d22a797b454abf7777c641a534a2bfd5d67f5d182c75a4a7fe9f3a31eb85afe533cc55edca86b9dfe6e8d66c2c00a90097ac0ff8ca06f00dd524018fb422ad68994ec537cd12ab10f4144a8f3290dae1a","size":859,"time":1002,"version":1,"versionHex":"1"},"id":1}"#
        );
    }

    #[test]
    fn verbose_block_error() {
        let client = BlockChainClient::new(ErrorBlockChainClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
             .handle_request_sync(
                 &(r#"
                    {
                    	"jsonrpc": "2.0",
                    	"method": "getblock",
                    	"params": ["000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd", true],
                    	"id": 1
                    }"#),
             )
             .unwrap();

        assert_eq!(
            &sample,
            r#"{"jsonrpc":"2.0","error":{"code":-32099,"message":"Block with given hash is not found","data":"000000006a625f06636b8bb6ac7b960a8d03705d1ace08b1a19da3fdcc99ddbd"},"id":1}"#
        );
    }
}
