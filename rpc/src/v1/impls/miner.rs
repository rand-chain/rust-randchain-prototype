use jsonrpc_core::Error;
use miner;
use sync;
use v1::traits::Miner;
use v1::types::{BlockTemplate, BlockTemplateRequest};

pub struct MinerClient<T: MinerClientCoreApi> {
    core: T,
}

pub trait MinerClientCoreApi: Send + Sync + 'static {
    fn get_block_template(&self) -> miner::BlockTemplate;
}

pub struct MinerClientCore {
    local_sync_node: sync::LocalNodeRef,
}

impl MinerClientCore {
    pub fn new(local_sync_node: sync::LocalNodeRef) -> Self {
        MinerClientCore {
            local_sync_node: local_sync_node,
        }
    }
}

impl MinerClientCoreApi for MinerClientCore {
    fn get_block_template(&self) -> miner::BlockTemplate {
        self.local_sync_node.get_block_template()
    }
}

impl<T> MinerClient<T>
where
    T: MinerClientCoreApi,
{
    pub fn new(core: T) -> Self {
        MinerClient { core: core }
    }
}

impl<T> Miner for MinerClient<T>
where
    T: MinerClientCoreApi,
{
    fn get_block_template(&self, _request: BlockTemplateRequest) -> Result<BlockTemplate, Error> {
        Ok(self.core.get_block_template().into())
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use jsonrpc_core::IoHandler;
    use miner;
    use primitives::hash::H256;
    use v1::traits::Miner;

    #[derive(Default)]
    struct SuccessMinerClientCore;

    impl MinerClientCoreApi for SuccessMinerClientCore {
        fn get_block_template(&self) -> miner::BlockTemplate {
            miner::BlockTemplate {
                version: 777,
                previous_header_hash: H256::from(1),
                time: 33,
                bits: 44.into(),
                height: 55,
            }
        }
    }

    #[test]
    fn getblocktemplate_accepted() {
        let client = MinerClient::new(SuccessMinerClientCore::default());
        let mut handler = IoHandler::new();
        handler.extend_with(client.to_delegate());

        let sample = handler
            .handle_request_sync(
                &(r#"
			{
				"jsonrpc": "2.0",
				"method": "getblocktemplate",
				"params": [{}],
				"id": 1
			}"#),
            )
            .unwrap();

        // direct hash is 0100000000000000000000000000000000000000000000000000000000000000
        // but client expects reverse hash
        assert_eq!(
            &sample,
            r#"{"jsonrpc":"2.0","result":{"bits":44,"coinbaseaux":null,"curtime":33,"height":55,"mintime":null,"mutable":null,"previousblockhash":"0000000000000000000000000000000000000000000000000000000000000001","rules":null,"target":"0000000000000000000000000000000000000000000000000000000000000000","vbavailable":null,"vbrequired":null,"version":777,"weightlimit":null},"id":1}"#
        );
    }
}
