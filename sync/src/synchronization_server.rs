use message::{common, types};
use parking_lot::{Condvar, Mutex};
use primitives::hash::H256;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use synchronization_executor::{Task, TaskExecutor};
use types::{BlockHeight, ExecutorRef, PeerIndex, PeersRef, RequestId, StorageRef};

/// Synchronization server task
#[derive(Debug, PartialEq)]
pub enum ServerTask {
    /// Serve 'getdata' request
    GetData(PeerIndex, types::GetData),
    /// Serve reversed 'getdata' request
    ReversedGetData(PeerIndex, types::GetData, types::NotFound),
    /// Serve 'getblocks' request
    GetBlocks(PeerIndex, types::GetBlocks),
    /// Serve 'getheaders' request
    GetHeaders(PeerIndex, types::GetHeaders, RequestId),
    /// Serve 'mempool' request
    Mempool(PeerIndex),
}

/// Synchronization server
pub trait Server: Send + Sync + 'static {
    /// Execute single synchronization task
    fn execute(&self, task: ServerTask);
    /// Called when connection is closed
    fn on_disconnect(&self, peer_index: PeerIndex);
}

/// Synchronization requests server
pub struct ServerImpl {
    queue_ready: Arc<Condvar>,
    queue: Arc<Mutex<ServerQueue>>,
    worker_thread: Option<thread::JoinHandle<()>>,
}

/// Server tasks queue
struct ServerQueue {
    is_stopping: AtomicBool,
    queue_ready: Arc<Condvar>,
    peers_queue: VecDeque<usize>,
    tasks_queue: HashMap<usize, VecDeque<ServerTask>>,
}

/// Server tasks executor
struct ServerTaskExecutor<T>
where
    T: TaskExecutor,
{
    /// Peers
    peers: PeersRef,
    /// Executor
    executor: ExecutorRef<T>,
    /// Storage reference
    storage: StorageRef,
}

impl Server for ServerImpl {
    fn execute(&self, task: ServerTask) {
        self.queue.lock().add_task(task);
    }

    fn on_disconnect(&self, peer_index: PeerIndex) {
        self.queue.lock().remove_peer_tasks(peer_index);
    }
}

impl ServerTask {
    pub fn peer_index(&self) -> PeerIndex {
        match *self {
            ServerTask::GetData(peer_index, _)
            | ServerTask::ReversedGetData(peer_index, _, _)
            | ServerTask::GetBlocks(peer_index, _)
            | ServerTask::GetHeaders(peer_index, _, _)
            | ServerTask::Mempool(peer_index) => peer_index,
        }
    }
}

impl ServerImpl {
    pub fn new<T: TaskExecutor>(peers: PeersRef, storage: StorageRef, executor: Arc<T>) -> Self {
        let executor = ServerTaskExecutor::new(peers, storage, executor);
        let queue_ready = Arc::new(Condvar::new());
        let queue = Arc::new(Mutex::new(ServerQueue::new(queue_ready.clone())));
        let mut server = ServerImpl {
            queue_ready: queue_ready.clone(),
            queue: queue.clone(),
            worker_thread: None,
        };
        server.worker_thread = Some(thread::spawn(move || {
            ServerImpl::server_worker(queue_ready, queue, executor);
        }));
        server
    }

    fn server_worker<T: TaskExecutor>(
        queue_ready: Arc<Condvar>,
        queue: Arc<Mutex<ServerQueue>>,
        executor: ServerTaskExecutor<T>,
    ) {
        loop {
            let task = {
                let mut queue = queue.lock();
                if queue.is_stopping.load(Ordering::SeqCst) {
                    break;
                }

                queue.next_task().or_else(|| {
                    queue_ready.wait(&mut queue);
                    queue.next_task()
                })
            };

            if let Some(task) = task {
                if let Some(task) = executor.execute(task) {
                    queue.lock().add_task_front(task);
                }
            }
        }
    }
}

impl Drop for ServerImpl {
    fn drop(&mut self) {
        if let Some(join_handle) = self.worker_thread.take() {
            self.queue.lock().is_stopping.store(true, Ordering::SeqCst);
            self.queue_ready.notify_one();
            join_handle.join().expect("Clean shutdown.");
        }
    }
}

impl ServerQueue {
    pub fn new(queue_ready: Arc<Condvar>) -> Self {
        ServerQueue {
            is_stopping: AtomicBool::new(false),
            queue_ready: queue_ready,
            peers_queue: VecDeque::new(),
            tasks_queue: HashMap::new(),
        }
    }

    pub fn next_task(&mut self) -> Option<ServerTask> {
        self.peers_queue.pop_front()
			.map(|peer_index| {
				let (peer_task, is_last_peer_task) = {
					let peer_tasks = self.tasks_queue.get_mut(&peer_index)
						.expect("entry from tasks_queue is removed when empty; when empty, peer is removed from peers_queue; qed");
					let peer_task = peer_tasks.pop_front()
						.expect("entry from peer_tasks is removed when empty; when empty, peer is removed from peers_queue; qed");
					(peer_task, peer_tasks.is_empty())
				};

				// remove if no tasks left || schedule otherwise
				if !is_last_peer_task {
					self.peers_queue.push_back(peer_index);
				} else {
					self.tasks_queue.remove(&peer_index);
				}

				peer_task
			})
    }

    pub fn add_task(&mut self, task: ServerTask) {
        let peer_index = task.peer_index();
        match self.tasks_queue.entry(peer_index) {
            Entry::Occupied(mut entry) => {
                let add_to_peers_queue = entry.get().is_empty();
                entry.get_mut().push_back(task);
                if add_to_peers_queue {
                    self.peers_queue.push_back(peer_index);
                }
            }
            Entry::Vacant(entry) => {
                let mut new_tasks = VecDeque::new();
                new_tasks.push_back(task);
                entry.insert(new_tasks);
                self.peers_queue.push_back(peer_index);
            }
        }
        self.queue_ready.notify_one();
    }

    pub fn add_task_front(&mut self, task: ServerTask) {
        let peer_index = task.peer_index();
        match self.tasks_queue.entry(peer_index) {
            Entry::Occupied(mut entry) => {
                let add_to_peers_queue = entry.get().is_empty();
                entry.get_mut().push_front(task);
                if add_to_peers_queue {
                    self.peers_queue.push_back(peer_index);
                }
            }
            Entry::Vacant(entry) => {
                let mut new_tasks = VecDeque::new();
                new_tasks.push_back(task);
                entry.insert(new_tasks);
                self.peers_queue.push_back(peer_index);
            }
        }
        self.queue_ready.notify_one();
    }

    pub fn remove_peer_tasks(&mut self, peer_index: PeerIndex) {
        if self.tasks_queue.remove(&peer_index).is_some() {
            let position = self.peers_queue.iter().position(|p| p == &peer_index)
				.expect("there are tasks for peer in tasks_queue; all tasks from tasks_queue are queued in peers_queue; qed");
            self.peers_queue.remove(position);
        }
    }
}

impl<TExecutor> ServerTaskExecutor<TExecutor>
where
    TExecutor: TaskExecutor,
{
    pub fn new(peers: PeersRef, storage: StorageRef, executor: ExecutorRef<TExecutor>) -> Self {
        ServerTaskExecutor {
            peers: peers,
            storage: storage,
            executor: executor,
        }
    }

    pub fn execute(&self, task: ServerTask) -> Option<ServerTask> {
        match task {
            ServerTask::GetData(peer_index, message) => {
                return self.serve_get_data(peer_index, message)
            }
            ServerTask::ReversedGetData(peer_index, message, notfound) => {
                return self.serve_reversed_get_data(peer_index, message, notfound)
            }
            ServerTask::GetBlocks(peer_index, message) => {
                self.serve_get_blocks(peer_index, message)
            }
            ServerTask::GetHeaders(peer_index, message, request_id) => {
                self.serve_get_headers(peer_index, message, request_id)
            }
            ServerTask::Mempool(peer_index) => self.serve_mempool(peer_index),
        }

        None
    }

    fn serve_get_data(
        &self,
        peer_index: PeerIndex,
        mut message: types::GetData,
    ) -> Option<ServerTask> {
        // getdata request is served by single item by just popping values from the back
        // of inventory vector
        // => to respond in given order, we have to reverse blocks inventory here
        message.inventory.reverse();
        // + while iterating by items, also accumulate unknown items to respond with notfound
        let notfound = types::NotFound {
            inventory: Vec::new(),
        };
        Some(ServerTask::ReversedGetData(peer_index, message, notfound))
    }

    fn serve_reversed_get_data(
        &self,
        peer_index: PeerIndex,
        mut message: types::GetData,
        mut notfound: types::NotFound,
    ) -> Option<ServerTask> {
        let next_item = match message.inventory.pop() {
            None => {
                if !notfound.inventory.is_empty() {
                    trace!(target: "sync", "'getdata' from peer#{} container contains {} unknown items", peer_index, notfound.inventory.len());
                    self.executor.execute(Task::NotFound(peer_index, notfound));
                }
                return None;
            }
            Some(next_item) => next_item,
        };

        match next_item.inv_type {
            common::InventoryType::MessageBlock => {
                if let Some(block) = self.storage.block(next_item.hash.clone().into()) {
                    trace!(target: "sync", "'getblocks' response to peer#{} is ready with block {}", peer_index, next_item.hash.to_reversed_str());
                    self.executor.execute(Task::Block(peer_index, block));
                } else {
                    notfound.inventory.push(next_item);
                }
            }
            common::InventoryType::Error => (),
        }

        Some(ServerTask::ReversedGetData(peer_index, message, notfound))
    }

    fn serve_get_blocks(&self, peer_index: PeerIndex, message: types::GetBlocks) {
        if let Some(block_height) =
            self.locate_best_common_block(&message.hash_stop, &message.block_locator_hashes)
        {
            let inventory: Vec<_> = (block_height + 1
                ..block_height + 1 + (types::GETBLOCKS_MAX_RESPONSE_HASHES as BlockHeight))
                .map(|block_height| self.storage.block_hash(block_height))
                .take_while(Option::is_some)
                .map(Option::unwrap)
                .take_while(|block_hash| block_hash != &message.hash_stop)
                .map(common::InventoryVector::block)
                .collect();
            // empty inventory messages are invalid according to regtests, while empty headers messages are valid
            if !inventory.is_empty() {
                trace!(target: "sync", "'getblocks' response to peer#{} is ready with {} hashes", peer_index, inventory.len());
                self.executor.execute(Task::Inventory(
                    peer_index,
                    types::Inv::with_inventory(inventory),
                ));
            } else {
                trace!(target: "sync", "'getblocks' request from peer#{} is ignored as there are no new blocks for peer", peer_index);
            }
        } else {
            self.peers
                .misbehaving(peer_index, "Got 'getblocks' message without known blocks");
            return;
        }
    }

    fn serve_get_headers(
        &self,
        peer_index: PeerIndex,
        message: types::GetHeaders,
        request_id: RequestId,
    ) {
        if let Some(block_height) =
            self.locate_best_common_block(&message.hash_stop, &message.block_locator_hashes)
        {
            let headers: Vec<_> = (block_height + 1
                ..block_height + 1 + (types::GETHEADERS_MAX_RESPONSE_HEADERS as BlockHeight))
                .map(|block_height| self.storage.block_hash(block_height))
                .take_while(Option::is_some)
                .map(Option::unwrap)
                .take_while(|block_hash| block_hash != &message.hash_stop)
                .map(|block_hash| self.storage.block_header(block_hash.into()))
                .take_while(Option::is_some)
                .map(Option::unwrap)
                .map(|h| h.raw)
                .collect();
            // empty inventory messages are invalid according to regtests, while empty headers messages are valid
            trace!(target: "sync", "'getheaders' response to peer#{} is ready with {} headers", peer_index, headers.len());
            self.executor.execute(Task::Headers(
                peer_index,
                types::Headers::with_headers(headers),
                Some(request_id),
            ));
        } else {
            self.peers
                .misbehaving(peer_index, "Got 'headers' message without known blocks");
            return;
        }
    }

    // TODO:
    fn serve_mempool(&self, peer_index: PeerIndex) {
        trace!(target: "sync", "'mempool' request from peer#{} is ignored as pool is empty", peer_index);
    }

    fn locate_best_common_block(&self, hash_stop: &H256, locator: &[H256]) -> Option<BlockHeight> {
        for block_hash in locator.iter().chain(&[hash_stop.clone()]) {
            if let Some(block_number) = self.storage.block_number(block_hash) {
                return Some(block_number);
            }

            // block with this hash is definitely not in the main chain (block_number has returned None)
            // but maybe it is in some fork? if so => we should find intersection with main chain
            // and this would be our best common block
            let mut block_hash = block_hash.clone();
            loop {
                let block_header = match self.storage.block_header(block_hash.into()) {
                    None => break,
                    Some(block_header) => block_header,
                };

                if let Some(block_number) = self
                    .storage
                    .block_number(&block_header.raw.previous_header_hash)
                {
                    return Some(block_number);
                }

                block_hash = block_header.raw.previous_header_hash;
            }
        }

        None
    }
}

#[cfg(test)]
pub mod tests {
    extern crate test_data;

    use super::{Server, ServerImpl, ServerTask};
    use db::BlockChainDatabase;
    use message::common::{InventoryType, InventoryVector};
    use message::types;
    use parking_lot::Mutex;
    use primitives::hash::H256;
    use std::mem::replace;
    use std::sync::Arc;
    use synchronization_executor::tests::DummyTaskExecutor;
    use synchronization_executor::Task;
    use synchronization_peers::PeersImpl;
    use types::{ExecutorRef, PeerIndex, PeersRef, StorageRef};

    pub struct DummyServer {
        tasks: Mutex<Vec<ServerTask>>,
    }

    impl DummyServer {
        pub fn new() -> Self {
            DummyServer {
                tasks: Mutex::new(Vec::new()),
            }
        }

        pub fn take_tasks(&self) -> Vec<ServerTask> {
            replace(&mut *self.tasks.lock(), Vec::new())
        }
    }

    impl Server for DummyServer {
        fn execute(&self, task: ServerTask) {
            self.tasks.lock().push(task);
        }

        fn on_disconnect(&self, _peer_index: PeerIndex) {}
    }

    fn create_synchronization_server() -> (
        StorageRef,
        ExecutorRef<DummyTaskExecutor>,
        PeersRef,
        ServerImpl,
    ) {
        let peers = Arc::new(PeersImpl::default());
        let storage = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let executor = DummyTaskExecutor::new();
        let server = ServerImpl::new(peers.clone(), storage.clone(), executor.clone());
        (storage, executor, peers, server)
    }

    #[test]
    fn server_getdata_responds_notfound_when_block_not_found() {
        let (_, executor, _, server) = create_synchronization_server();
        // when asking for unknown block
        let inventory = vec![InventoryVector {
            inv_type: InventoryType::MessageBlock,
            hash: H256::default(),
        }];
        server.execute(ServerTask::GetData(
            0,
            types::GetData::with_inventory(inventory.clone()),
        ));
        // => respond with notfound
        let tasks = DummyTaskExecutor::wait_tasks(executor);
        assert_eq!(
            tasks,
            vec![Task::NotFound(
                0,
                types::NotFound::with_inventory(inventory)
            )]
        );
    }

    #[test]
    fn server_getdata_responds_block_when_block_is_found() {
        let (_, executor, _, server) = create_synchronization_server();
        // when asking for known block
        let inventory = vec![InventoryVector {
            inv_type: InventoryType::MessageBlock,
            hash: test_data::genesis().hash(),
        }];
        server.execute(ServerTask::GetData(
            0,
            types::GetData::with_inventory(inventory.clone()),
        ));
        // => respond with block
        let tasks = DummyTaskExecutor::wait_tasks(executor);
        assert_eq!(tasks, vec![Task::Block(0, test_data::genesis().into())]);
    }

    #[test]
    fn server_getblocks_do_not_responds_inventory_when_synchronized() {
        let (_, executor, _, server) = create_synchronization_server();
        // when asking for blocks hashes
        let genesis_block_hash = test_data::genesis().hash();
        server.execute(ServerTask::GetBlocks(
            0,
            types::GetBlocks {
                version: 0,
                block_locator_hashes: vec![genesis_block_hash.clone()],
                hash_stop: H256::default(),
            },
        ));
        // => empty response
        let tasks = DummyTaskExecutor::wait_tasks_for(executor, 100); // TODO: get rid of explicit timeout
        assert_eq!(tasks, vec![]);
    }

    #[test]
    fn server_getblocks_responds_inventory_when_have_unknown_blocks() {
        let (storage, executor, _, server) = create_synchronization_server();
        storage
            .insert(test_data::block_h1().into())
            .expect("Db write error");
        storage.canonize(&test_data::block_h1().hash()).unwrap();
        // when asking for blocks hashes
        server.execute(ServerTask::GetBlocks(
            0,
            types::GetBlocks {
                version: 0,
                block_locator_hashes: vec![test_data::genesis().hash()],
                hash_stop: H256::default(),
            },
        ));
        // => responds with inventory
        let inventory = vec![InventoryVector {
            inv_type: InventoryType::MessageBlock,
            hash: test_data::block_h1().hash(),
        }];
        let tasks = DummyTaskExecutor::wait_tasks(executor);
        assert_eq!(
            tasks,
            vec![Task::Inventory(0, types::Inv::with_inventory(inventory))]
        );
    }

    #[test]
    fn server_getheaders_do_not_responds_headers_when_synchronized() {
        let (_, executor, _, server) = create_synchronization_server();
        // when asking for blocks hashes
        let genesis_block_hash = test_data::genesis().hash();
        let dummy_id = 6;
        server.execute(ServerTask::GetHeaders(
            0,
            types::GetHeaders {
                version: 0,
                block_locator_hashes: vec![genesis_block_hash.clone()],
                hash_stop: H256::default(),
            },
            dummy_id,
        ));
        // => no response
        let tasks = DummyTaskExecutor::wait_tasks_for(executor, 100); // TODO: get rid of explicit timeout
        assert_eq!(
            tasks,
            vec![Task::Headers(
                0,
                types::Headers::with_headers(vec![]),
                Some(dummy_id)
            )]
        );
    }

    #[test]
    fn server_getheaders_responds_headers_when_have_unknown_blocks() {
        let (storage, executor, _, server) = create_synchronization_server();
        storage
            .insert(test_data::block_h1().into())
            .expect("Db write error");
        storage.canonize(&test_data::block_h1().hash()).unwrap();
        // when asking for blocks hashes
        let dummy_id = 0;
        server.execute(ServerTask::GetHeaders(
            0,
            types::GetHeaders {
                version: 0,
                block_locator_hashes: vec![test_data::genesis().hash()],
                hash_stop: H256::default(),
            },
            dummy_id,
        ));
        // => responds with headers
        let headers = vec![test_data::block_h1().block_header];
        let tasks = DummyTaskExecutor::wait_tasks(executor);
        assert_eq!(
            tasks,
            vec![Task::Headers(
                0,
                types::Headers::with_headers(headers),
                Some(dummy_id)
            )]
        );
    }

    #[test]
    fn server_mempool_do_not_responds_inventory_when_empty_memory_pool() {
        let (_, executor, _, server) = create_synchronization_server();
        // when asking for memory pool transactions ids
        server.execute(ServerTask::Mempool(0));
        // => no response
        let tasks = DummyTaskExecutor::wait_tasks_for(executor, 100); // TODO: get rid of explicit timeout
        assert_eq!(tasks, vec![]);
    }

    #[test]
    fn server_responds_with_nonempty_inventory_when_getdata_stop_hash_filled() {
        let (storage, executor, _, server) = create_synchronization_server();
        {
            storage
                .insert(test_data::block_h1().into())
                .expect("no error");
            storage.canonize(&test_data::block_h1().hash()).unwrap();
        }
        // when asking with stop_hash
        server.execute(ServerTask::GetBlocks(
            0,
            types::GetBlocks {
                version: 0,
                block_locator_hashes: vec![],
                hash_stop: test_data::genesis().hash(),
            },
        ));
        // => respond with next block
        let inventory = vec![InventoryVector {
            inv_type: InventoryType::MessageBlock,
            hash: test_data::block_h1().hash(),
        }];
        let tasks = DummyTaskExecutor::wait_tasks(executor);
        assert_eq!(
            tasks,
            vec![Task::Inventory(0, types::Inv::with_inventory(inventory))]
        );
    }

    #[test]
    fn server_responds_with_nonempty_headers_when_getdata_stop_hash_filled() {
        let (storage, executor, _, server) = create_synchronization_server();
        {
            storage
                .insert(test_data::block_h1().into())
                .expect("no error");
            storage.canonize(&test_data::block_h1().hash()).unwrap();
        }
        // when asking with stop_hash
        let dummy_id = 6;
        server.execute(ServerTask::GetHeaders(
            0,
            types::GetHeaders {
                version: 0,
                block_locator_hashes: vec![],
                hash_stop: test_data::genesis().hash(),
            },
            dummy_id,
        ));
        // => respond with next block
        let headers = vec![test_data::block_h1().block_header];
        let tasks = DummyTaskExecutor::wait_tasks(executor);
        assert_eq!(
            tasks,
            vec![Task::Headers(
                0,
                types::Headers::with_headers(headers),
                Some(dummy_id)
            )]
        );
    }
}
