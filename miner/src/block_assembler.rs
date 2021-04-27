use network::Network;
use primitives::compact::Compact;
use primitives::hash::H256;
use storage::SharedStore;
use verification::work_required;

const BLOCK_VERSION: u32 = 0x20000000;
// TODO:
// const BLOCK_HEADER_SIZE: u32 = 4 + 32 + 32 + 4 + 4 + 4;

pub struct BlockTemplate {
    /// Version
    pub version: u32,
    /// The hash of previous block
    pub previous_header_hash: H256,
    /// The current time as seen by the server
    pub time: u32,
    /// The compressed difficulty
    pub bits: Compact,
    /// Block height
    pub height: u32,
}

/// Block assembler
pub struct BlockAssembler {}

impl BlockAssembler {
    pub fn create_new_block(
        &self,
        store: &SharedStore,
        time: u32,
        network: &Network,
    ) -> BlockTemplate {
        // get best block
        // take it's hash && height
        let best_block = store.best_block();
        let previous_header_hash = best_block.hash;
        let height = best_block.number + 1;
        let bits = work_required(
            previous_header_hash.clone(),
            time,
            height,
            store.as_block_header_provider(),
            network,
        );
        let version = BLOCK_VERSION;

        BlockTemplate {
            version: version,
            previous_header_hash: previous_header_hash,
            time: time,
            bits: bits,
            height: height,
        }
    }
}
