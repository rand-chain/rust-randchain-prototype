use chain::{IndexedBlock, IndexedBlockHeader};
use futures::{finished, lazy};
use message::types;
use miner::BlockAssembler;
use miner::BlockTemplate;
use network::Network;
use std::sync::Arc;
use synchronization_client::Client;
use synchronization_peers::{BlockAnnouncementType, TransactionAnnouncementType};
use synchronization_server::{Server, ServerTask};
use time;
use types::{
    ClientRef, PeerIndex, PeersRef, RequestId, ServerRef, StorageRef, SyncListenerRef,
    SynchronizationStateRef,
};

/// Local synchronization node
pub struct LocalNode<U: Server, V: Client> {
    /// Network we are working on
    network: Network,
    /// Storage reference
    storage: StorageRef,
    /// Synchronization peers
    peers: PeersRef,
    /// Shared synchronization state
    state: SynchronizationStateRef,
    /// Synchronization process
    client: ClientRef<V>,
    /// Synchronization server
    server: ServerRef<U>,
}

impl<U, V> LocalNode<U, V>
where
    U: Server,
    V: Client,
{
    /// Create new synchronization node
    #[cfg_attr(feature = "cargo-clippy", allow(too_many_arguments))]
    pub fn new(
        network: Network,
        storage: StorageRef,
        peers: PeersRef,
        state: SynchronizationStateRef,
        client: ClientRef<V>,
        server: ServerRef<U>,
    ) -> Self {
        LocalNode {
            network: network,
            storage: storage,
            peers: peers,
            state: state,
            client: client,
            server: server,
        }
    }

    /// Return shared reference to synchronization state.
    pub fn sync_state(&self) -> SynchronizationStateRef {
        self.state.clone()
    }

    /// When new peer connects to the node
    pub fn on_connect(&self, peer_index: PeerIndex, peer_name: String, version: types::Version) {
        trace!(target: "sync", "Starting new sync session with peer#{}: {}", peer_index, peer_name);

        // light clients may not want transactions broadcasting until filter for connection is set
        if !version.relay_transactions() {
            self.peers.set_transaction_announcement_type(
                peer_index,
                TransactionAnnouncementType::DoNotAnnounce,
            );
        }

        // start synchronization session with peer
        self.client.on_connect(peer_index);
    }

    /// When peer disconnects
    pub fn on_disconnect(&self, peer_index: PeerIndex) {
        trace!(target: "sync", "Stopping sync session with peer#{}", peer_index);

        // stop synchronization session with peer
        self.client.on_disconnect(peer_index);
    }

    /// When inventory message is received
    pub fn on_inventory(&self, peer_index: PeerIndex, message: types::Inv) {
        trace!(target: "sync", "Got `inventory` message from peer#{}. Inventory len: {}", peer_index, message.inventory.len());
        self.client.on_inventory(peer_index, message);
    }

    /// When headers message is received
    pub fn on_headers(&self, peer_index: PeerIndex, headers: Vec<IndexedBlockHeader>) {
        trace!(target: "sync", "Got `headers` message from peer#{}. Headers len: {}", peer_index, headers.len());
        self.client.on_headers(peer_index, headers);
    }

    /// When block is received
    pub fn on_block(&self, peer_index: PeerIndex, block: IndexedBlock) {
        trace!(target: "sync", "Got `block` message from peer#{}. Block hash: {}", peer_index, block.header.hash.to_reversed_str());
        self.client.on_block(peer_index, block);
    }

    /// When notfound is received
    pub fn on_notfound(&self, peer_index: PeerIndex, message: types::NotFound) {
        trace!(target: "sync", "Got `notfound` message from peer#{}", peer_index);
        self.client.on_notfound(peer_index, message);
    }

    /// When peer is requesting for items
    pub fn on_getdata(&self, peer_index: PeerIndex, message: types::GetData) {
        trace!(target: "sync", "Got `getdata` message from peer#{}. Inventory len: {}", peer_index, message.inventory.len());
        self.server
            .execute(ServerTask::GetData(peer_index, message));
    }

    /// When peer is requesting for known blocks hashes
    pub fn on_getblocks(&self, peer_index: PeerIndex, message: types::GetBlocks) {
        trace!(target: "sync", "Got `getblocks` message from peer#{}", peer_index);
        self.server
            .execute(ServerTask::GetBlocks(peer_index, message));
    }

    /// When peer is requesting for known blocks headers
    pub fn on_getheaders(&self, peer_index: PeerIndex, message: types::GetHeaders, id: RequestId) {
        trace!(target: "sync", "Got `getheaders` message from peer#{}", peer_index);

        // simulating randchaind for passing tests: if we are in nearly-saturated state
        // and peer, which has just provided a new blocks to us, is asking for headers
        // => do not serve getheaders until we have fully process his blocks + wait until headers are served before returning
        let server = Arc::downgrade(&self.server);
        let server_task = ServerTask::GetHeaders(peer_index, message, id);
        let lazy_server_task = lazy(move || {
            server.upgrade().map(|s| s.execute(server_task));
            finished::<(), ()>(())
        });
        self.client
            .after_peer_nearly_blocks_verified(peer_index, Box::new(lazy_server_task));
    }

    /// When peer asks us to announce new blocks using headers message
    pub fn on_sendheaders(&self, peer_index: PeerIndex, _message: types::SendHeaders) {
        trace!(target: "sync", "Got `sendheaders` message from peer#{}", peer_index);
        self.peers
            .set_block_announcement_type(peer_index, BlockAnnouncementType::SendHeaders);
    }

    /// Get block template for mining
    pub fn get_block_template(&self) -> BlockTemplate {
        let block_assembler = BlockAssembler {};
        block_assembler.create_new_block(&self.storage, time::get_time().sec as u32, &self.network)
    }

    /// Install synchronization events listener
    pub fn install_sync_listener(&self, listener: SyncListenerRef) {
        self.client.install_sync_listener(listener);
    }
}

#[cfg(test)]
pub mod tests {
    extern crate test_data;

    use super::LocalNode;
    use db::BlockChainDatabase;
    use message::common::{InventoryType, InventoryVector};
    use message::types;
    use network::Network;
    use std::sync::Arc;
    use synchronization_chain::Chain;
    use synchronization_client::SynchronizationClient;
    use synchronization_client_core::{Config, CoreVerificationSink, SynchronizationClientCore};
    use synchronization_executor::tests::DummyTaskExecutor;
    use synchronization_peers::PeersImpl;
    use synchronization_server::tests::DummyServer;
    use synchronization_server::ServerTask;
    use synchronization_verifier::tests::DummyVerifier;
    use types::SynchronizationStateRef;
    use utils::SynchronizationState;
    use verification::BackwardsCompatibleChainVerifier as ChainVerifier;

    fn create_local_node(
        verifier: Option<DummyVerifier>,
    ) -> (
        Arc<DummyTaskExecutor>,
        Arc<DummyServer>,
        LocalNode<DummyServer, SynchronizationClient<DummyTaskExecutor, DummyVerifier>>,
    ) {
        let storage = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let sync_state =
            SynchronizationStateRef::new(SynchronizationState::with_storage(storage.clone()));
        let chain = Chain::new(storage.clone());
        let sync_peers = Arc::new(PeersImpl::default());
        let executor = DummyTaskExecutor::new();
        let server = Arc::new(DummyServer::new());
        let config = Config {
            // close_connection_on_bad_block: true,
            close_connection_on_bad_block: false,
        };
        let chain_verifier = Arc::new(ChainVerifier::new(storage.clone(), Network::Mainnet));
        let client_core = SynchronizationClientCore::new(
            config,
            sync_state.clone(),
            sync_peers.clone(),
            executor.clone(),
            chain,
            chain_verifier,
        );
        let mut verifier = match verifier {
            Some(verifier) => verifier,
            None => DummyVerifier::default(),
        };
        verifier.set_sink(Arc::new(CoreVerificationSink::new(client_core.clone())));
        let client = SynchronizationClient::new(client_core, verifier);
        let local_node = LocalNode::new(
            Network::Mainnet,
            storage,
            sync_peers,
            sync_state,
            client,
            server.clone(),
        );
        (executor, server, local_node)
    }

    #[test]
    fn local_node_serves_block() {
        let (_, server, local_node) = create_local_node(None);
        let peer_index = 0;
        local_node.on_connect(peer_index, "test".into(), types::Version::default());
        // peer requests genesis block
        let genesis_block_hash = test_data::genesis().hash();
        let inventory = vec![InventoryVector {
            inv_type: InventoryType::MessageBlock,
            hash: genesis_block_hash.clone(),
        }];
        local_node.on_getdata(
            peer_index,
            types::GetData {
                inventory: inventory.clone(),
            },
        );
        // => `getdata` is served
        let tasks = server.take_tasks();
        assert_eq!(
            tasks,
            vec![ServerTask::GetData(
                peer_index,
                types::GetData::with_inventory(inventory)
            )]
        );
    }
}
