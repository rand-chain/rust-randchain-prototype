use chain::IndexedBlock;
use message::common::InventoryVector;
use message::types;
use std::sync::Arc;
use synchronization_peers::BlockAnnouncementType;
use types::{PeerIndex, PeersRef, RequestId};
use utils::KnownHashType;

/// Synchronization task executor
pub trait TaskExecutor: Send + Sync + 'static {
    fn execute(&self, task: Task);
}

/// Synchronization task for the peer.
#[derive(Debug, PartialEq)]
pub enum Task {
    /// Notify io about ignored request
    Ignore(PeerIndex, RequestId),
    /// Request unknown items from peer
    GetData(PeerIndex, types::GetData),
    /// Get headers
    GetHeaders(PeerIndex, types::GetHeaders),
    /// Send block
    Block(PeerIndex, IndexedBlock),
    /// Send notfound
    NotFound(PeerIndex, types::NotFound),
    /// Send inventory
    Inventory(PeerIndex, types::Inv),
    /// Send headers
    Headers(PeerIndex, types::Headers, Option<RequestId>),
    /// Relay new block to peers
    RelayNewBlock(IndexedBlock),
}

/// Synchronization tasks executor
pub struct LocalSynchronizationTaskExecutor {
    /// Active synchronization peers
    peers: PeersRef,
}

impl LocalSynchronizationTaskExecutor {
    pub fn new(peers: PeersRef) -> Arc<Self> {
        Arc::new(LocalSynchronizationTaskExecutor { peers: peers })
    }

    fn execute_ignore(&self, peer_index: PeerIndex, request_id: RequestId) {
        if let Some(connection) = self.peers.connection(peer_index) {
            trace!(target: "sync", "Ignoring request {} from peer#{}", request_id, peer_index);
            connection.ignored(request_id);
        }
    }

    fn execute_getdata(&self, peer_index: PeerIndex, getdata: types::GetData) {
        if let Some(connection) = self.peers.connection(peer_index) {
            trace!(target: "sync", "Querying {} unknown items from peer#{}", getdata.inventory.len(), peer_index);
            connection.send_getdata(&getdata);
        }
    }

    fn execute_getheaders(&self, peer_index: PeerIndex, getheaders: types::GetHeaders) {
        if let Some(connection) = self.peers.connection(peer_index) {
            if !getheaders.block_locator_hashes.is_empty() {
                trace!(target: "sync", "Querying headers starting with {} unknown items from peer#{}", getheaders.block_locator_hashes[0].to_reversed_str(), peer_index);
            }
            connection.send_getheaders(&getheaders);
        }
    }

    fn execute_block(&self, peer_index: PeerIndex, block: IndexedBlock) {
        if let Some(connection) = self.peers.connection(peer_index) {
            trace!(target: "sync", "Sending block {} to peer#{}", block.hash().to_reversed_str(), peer_index);
            self.peers
                .hash_known_as(peer_index, block.hash().clone(), KnownHashType::Block);
            let block = types::Block {
                block: block.to_raw_block(),
            };
            connection.send_block(&block);
        }
    }

    fn execute_notfound(&self, peer_index: PeerIndex, notfound: types::NotFound) {
        if let Some(connection) = self.peers.connection(peer_index) {
            trace!(target: "sync", "Sending notfound to peer#{} with {} items", peer_index, notfound.inventory.len());
            connection.send_notfound(&notfound);
        }
    }

    fn execute_inventory(&self, peer_index: PeerIndex, inventory: types::Inv) {
        if let Some(connection) = self.peers.connection(peer_index) {
            trace!(target: "sync", "Sending inventory to peer#{} with {} items", peer_index, inventory.inventory.len());
            connection.send_inventory(&inventory);
        }
    }

    fn execute_headers(
        &self,
        peer_index: PeerIndex,
        headers: types::Headers,
        request_id: Option<RequestId>,
    ) {
        if let Some(connection) = self.peers.connection(peer_index) {
            trace!(target: "sync", "Sending headers to peer#{} with {} items", peer_index, headers.headers.len());
            match request_id {
                Some(request_id) => connection.respond_headers(&headers, request_id),
                None => connection.send_headers(&headers),
            }
        }
    }

    fn execute_relay_block(&self, block: IndexedBlock) {
        for peer_index in self.peers.enumerate() {
            match self.peers.filter_block(peer_index, &block) {
                BlockAnnouncementType::SendInventory => {
                    self.execute_inventory(
                        peer_index,
                        types::Inv::with_inventory(vec![InventoryVector::block(
                            block.hash().clone(),
                        )]),
                    );
                }
                BlockAnnouncementType::SendHeaders => {
                    self.execute_headers(
                        peer_index,
                        types::Headers::with_headers(vec![block.header.raw.clone()]),
                        None,
                    );
                }
                BlockAnnouncementType::DoNotAnnounce => (),
            }
        }
    }
}

impl TaskExecutor for LocalSynchronizationTaskExecutor {
    fn execute(&self, task: Task) {
        match task {
            Task::Ignore(peer_index, request_id) => self.execute_ignore(peer_index, request_id),
            Task::GetData(peer_index, getdata) => self.execute_getdata(peer_index, getdata),
            Task::GetHeaders(peer_index, getheaders) => {
                self.execute_getheaders(peer_index, getheaders)
            }
            Task::Block(peer_index, block) => self.execute_block(peer_index, block),
            Task::NotFound(peer_index, notfound) => self.execute_notfound(peer_index, notfound),
            Task::Inventory(peer_index, inventory) => self.execute_inventory(peer_index, inventory),
            Task::Headers(peer_index, headers, request_id) => {
                self.execute_headers(peer_index, headers, request_id)
            }
            Task::RelayNewBlock(block) => self.execute_relay_block(block),
        }
    }
}

#[cfg(test)]
pub mod tests {
    extern crate test_data;

    use super::*;
    use inbound_connection::tests::DummyOutboundSyncConnection;
    use message::Services;
    use parking_lot::{Condvar, Mutex};
    use std::sync::Arc;
    use std::time;
    use synchronization_peers::{BlockAnnouncementType, PeersContainer, PeersImpl, PeersOptions};

    pub struct DummyTaskExecutor {
        tasks: Mutex<Vec<Task>>,
        waiter: Arc<Condvar>,
    }

    impl DummyTaskExecutor {
        pub fn new() -> Arc<Self> {
            Arc::new(DummyTaskExecutor {
                tasks: Mutex::new(Vec::new()),
                waiter: Arc::new(Condvar::new()),
            })
        }

        pub fn wait_tasks_for(executor: Arc<Self>, timeout_ms: u64) -> Vec<Task> {
            {
                let mut tasks = executor.tasks.lock();
                if tasks.is_empty() {
                    let waiter = executor.waiter.clone();
                    waiter
                        .wait_for(&mut tasks, time::Duration::from_millis(timeout_ms))
                        .timed_out();
                }
            }
            executor.take_tasks()
        }

        pub fn wait_tasks(executor: Arc<Self>) -> Vec<Task> {
            DummyTaskExecutor::wait_tasks_for(executor, 1000)
        }

        pub fn take_tasks(&self) -> Vec<Task> {
            let mut tasks = self.tasks.lock();
            let tasks = tasks.drain(..).collect();
            tasks
        }
    }

    impl TaskExecutor for DummyTaskExecutor {
        fn execute(&self, task: Task) {
            self.tasks.lock().push(task);
            self.waiter.notify_one();
        }
    }

    #[test]
    fn relay_new_block_after_sendheaders() {
        let peers = Arc::new(PeersImpl::default());
        let executor = LocalSynchronizationTaskExecutor::new(peers.clone());

        let c1 = DummyOutboundSyncConnection::new();
        peers.insert(1, Services::default(), c1.clone());
        let c2 = DummyOutboundSyncConnection::new();
        peers.insert(2, Services::default(), c2.clone());
        peers.set_block_announcement_type(2, BlockAnnouncementType::SendHeaders);

        executor.execute(Task::RelayNewBlock(test_data::genesis().into()));
        assert_eq!(
            *c1.messages
                .lock()
                .entry("inventory".to_owned())
                .or_insert(0),
            1
        );
        assert_eq!(
            *c2.messages.lock().entry("headers".to_owned()).or_insert(0),
            1
        );
    }
}
