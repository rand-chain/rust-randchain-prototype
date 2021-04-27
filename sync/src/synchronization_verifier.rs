use chain::IndexedBlock;
use network::Network;
use parking_lot::Mutex;
use primitives::hash::H256;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use types::StorageRef;
use verification::{
    BackwardsCompatibleChainVerifier as ChainVerifier, Error as VerificationError,
    VerificationLevel, Verify as VerificationVerify,
};
use VerificationParameters;

/// Block verification events sink
pub trait BlockVerificationSink: Send + Sync + 'static {
    /// When block verification has completed successfully.
    fn on_block_verification_success(&self, block: IndexedBlock) -> Option<Vec<VerificationTask>>;
    /// When block verification has failed.
    fn on_block_verification_error(&self, err: &str, hash: &H256);
}

/// Verification events sink
pub trait VerificationSink: BlockVerificationSink {}

/// Verification thread tasks
#[derive(Debug)]
pub enum VerificationTask {
    /// Verify single block
    VerifyBlock(IndexedBlock),
    /// Stop verification thread
    Stop,
}

/// Synchronization verifier
pub trait Verifier: Send + Sync + 'static {
    /// Verify block
    fn verify_block(&self, block: IndexedBlock);
}

/// Asynchronous synchronization verifier
pub struct AsyncVerifier {
    /// Verification work transmission channel.
    verification_work_sender: Mutex<Sender<VerificationTask>>,
    /// Verification thread.
    verification_worker_thread: Option<thread::JoinHandle<()>>,
}

/// Chain verifier wrapper to deal with verification parameters.
pub struct ChainVerifierWrapper {
    /// Original verifier.
    pub verifier: Arc<ChainVerifier>,
    /// Verification parameters.
    verification_params: VerificationParameters,
    /// Is verification edge passed.
    pub enforce_full_verification: AtomicBool,
}

impl ChainVerifierWrapper {
    /// Create new chain verifier wrapper.
    pub fn new(
        verifier: Arc<ChainVerifier>,
        storage: &StorageRef,
        verification_params: VerificationParameters,
    ) -> Self {
        let enforce_full_verification = AtomicBool::new(
            storage.contains_block(verification_params.verification_edge.clone().into()),
        );
        ChainVerifierWrapper {
            verifier: verifier,
            verification_params: verification_params,
            enforce_full_verification: enforce_full_verification,
        }
    }

    /// Verify block.
    pub fn verify_block(&self, block: &IndexedBlock) -> Result<(), VerificationError> {
        let enforce_full_verification =
            if block.hash() == &self.verification_params.verification_edge {
                self.enforce_full_verification
                    .store(true, Ordering::Relaxed);
                true
            } else {
                self.enforce_full_verification.load(Ordering::Relaxed)
            };
        let verification_level = if enforce_full_verification {
            VerificationLevel::Full
        } else {
            self.verification_params.verification_level
        };

        self.verifier.verify(verification_level, block)
    }
}

impl VerificationTask {}

impl AsyncVerifier {
    /// Create new async verifier
    pub fn new<T: VerificationSink>(
        verifier: Arc<ChainVerifier>,
        storage: StorageRef,
        sink: Arc<T>,
        verification_params: VerificationParameters,
    ) -> Self {
        let (verification_work_sender, verification_work_receiver) = channel();
        AsyncVerifier {
            verification_work_sender: Mutex::new(verification_work_sender),
            verification_worker_thread: Some(
                thread::Builder::new()
                    .name("Sync verification thread".to_string())
                    .spawn(move || {
                        let verifier =
                            ChainVerifierWrapper::new(verifier, &storage, verification_params);
                        AsyncVerifier::verification_worker_proc(
                            sink,
                            verifier,
                            verification_work_receiver,
                        )
                    })
                    .expect("Error creating sync verification thread"),
            ),
        }
    }

    /// Thread procedure for handling verification tasks
    fn verification_worker_proc<T: VerificationSink>(
        sink: Arc<T>,
        verifier: ChainVerifierWrapper,
        work_receiver: Receiver<VerificationTask>,
    ) {
        while let Ok(task) = work_receiver.recv() {
            if !AsyncVerifier::execute_single_task(&sink, &verifier, task) {
                break;
            }
        }

        trace!(target: "sync", "Stopping sync verification thread");
    }

    /// Execute single verification task
    pub fn execute_single_task<T: VerificationSink>(
        sink: &Arc<T>,
        verifier: &ChainVerifierWrapper,
        task: VerificationTask,
    ) -> bool {
        // block verification && insertion can lead to reorganization
        // => transactions from decanonized blocks should be put back to the MemoryPool
        // => they must be verified again
        // => here's sub-tasks queue
        let mut tasks_queue: VecDeque<VerificationTask> = VecDeque::new();
        tasks_queue.push_back(task);

        while let Some(task) = tasks_queue.pop_front() {
            match task {
                VerificationTask::VerifyBlock(block) => {
                    // verify block
                    match verifier.verify_block(&block) {
                        Ok(_) => {
                            if let Some(tasks) = sink.on_block_verification_success(block) {
                                tasks_queue.extend(tasks);
                            }
                        }
                        Err(e) => {
                            sink.on_block_verification_error(&format!("{:?}", e), block.hash())
                        }
                    }
                }
                VerificationTask::Stop => return false,
            }
        }

        true
    }
}

impl Drop for AsyncVerifier {
    fn drop(&mut self) {
        if let Some(join_handle) = self.verification_worker_thread.take() {
            {
                let verification_work_sender = self.verification_work_sender.lock();
                // ignore send error here <= destructing anyway
                let _ = verification_work_sender.send(VerificationTask::Stop);
            }
            join_handle.join().expect("Clean shutdown.");
        }
    }
}

impl Verifier for AsyncVerifier {
    /// Verify block
    fn verify_block(&self, block: IndexedBlock) {
        self.verification_work_sender
            .lock()
            .send(VerificationTask::VerifyBlock(block))
            .expect("Verification thread have the same lifetime as `AsyncVerifier`");
    }
}

/// Synchronous synchronization verifier
pub struct SyncVerifier<T: VerificationSink> {
    /// Verifier
    verifier: ChainVerifierWrapper,
    /// Verification sink
    sink: Arc<T>,
}

impl<T> SyncVerifier<T>
where
    T: VerificationSink,
{
    /// Create new sync verifier
    pub fn new(
        network: Network,
        storage: StorageRef,
        sink: Arc<T>,
        verification_params: VerificationParameters,
    ) -> Self {
        let verifier = ChainVerifier::new(storage.clone(), network);
        let verifier = ChainVerifierWrapper::new(Arc::new(verifier), &storage, verification_params);
        SyncVerifier {
            verifier: verifier,
            sink: sink,
        }
    }
}

impl<T> Verifier for SyncVerifier<T>
where
    T: VerificationSink,
{
    /// Verify block
    fn verify_block(&self, block: IndexedBlock) {
        match self.verifier.verify_block(&block) {
            Ok(_) => {
                // SyncVerifier is used for bulk blocks import only
                // => there are no memory pool
                // => we could ignore decanonized transactions
                self.sink.on_block_verification_success(block);
            }
            Err(e) => self
                .sink
                .on_block_verification_error(&format!("{:?}", e), block.hash()),
        }
    }
}

#[cfg(test)]
pub mod tests {
    extern crate test_data;

    use super::{
        AsyncVerifier, BlockVerificationSink, ChainVerifierWrapper, VerificationTask, Verifier,
    };
    use chain::IndexedBlock;
    use db::BlockChainDatabase;
    use network::Network;
    use primitives::hash::H256;
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use synchronization_client_core::CoreVerificationSink;
    use synchronization_executor::tests::DummyTaskExecutor;
    use types::StorageRef;
    use verification::{BackwardsCompatibleChainVerifier as ChainVerifier, VerificationLevel};
    use VerificationParameters;

    #[derive(Default)]
    pub struct DummyVerifier {
        sink: Option<Arc<CoreVerificationSink<DummyTaskExecutor>>>,
        errors: HashMap<H256, String>,
        actual_checks: HashSet<H256>,
        storage: Option<StorageRef>,
        verifier: Option<ChainVerifierWrapper>,
    }

    impl DummyVerifier {
        pub fn set_sink(&mut self, sink: Arc<CoreVerificationSink<DummyTaskExecutor>>) {
            self.sink = Some(sink);
        }

        pub fn set_storage(&mut self, storage: StorageRef) {
            self.storage = Some(storage);
        }

        pub fn set_verifier(&mut self, verifier: Arc<ChainVerifier>) {
            self.verifier = Some(ChainVerifierWrapper::new(
                verifier,
                self.storage.as_ref().unwrap(),
                VerificationParameters {
                    verification_level: VerificationLevel::Full,
                    verification_edge: 0u8.into(),
                },
            ));
        }

        pub fn error_when_verifying(&mut self, hash: H256, err: &str) {
            self.errors.insert(hash, err.into());
        }

        pub fn _actual_check_when_verifying(&mut self, hash: H256) {
            self.actual_checks.insert(hash);
        }
    }

    impl Verifier for DummyVerifier {
        fn verify_block(&self, block: IndexedBlock) {
            match self.sink {
                Some(ref sink) => match self.errors.get(&block.hash()) {
                    Some(err) => sink.on_block_verification_error(&err, &block.hash()),
                    None => {
                        if self.actual_checks.contains(block.hash()) {
                            AsyncVerifier::execute_single_task(
                                sink,
                                self.verifier.as_ref().unwrap(),
                                VerificationTask::VerifyBlock(block),
                            );
                        } else {
                            sink.on_block_verification_success(block);
                        }
                    }
                },
                None => panic!("call set_sink"),
            }
        }
    }

    #[test]
    fn verifier_wrapper_switches_to_full_mode() {
        let storage: StorageRef = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let verifier = Arc::new(ChainVerifier::new(storage.clone(), Network::Unitest));

        // switching to full verification when block is already in db
        assert_eq!(
            ChainVerifierWrapper::new(
                verifier.clone(),
                &storage,
                VerificationParameters {
                    verification_level: VerificationLevel::NoVerification,
                    verification_edge: test_data::genesis().hash(),
                }
            )
            .enforce_full_verification
            .load(Ordering::Relaxed),
            true
        );

        // switching to full verification when block with given hash is coming
        let wrapper = ChainVerifierWrapper::new(
            verifier,
            &storage,
            VerificationParameters {
                verification_level: VerificationLevel::NoVerification,
                verification_edge: test_data::block_h1().hash(),
            },
        );
        assert_eq!(
            wrapper.enforce_full_verification.load(Ordering::Relaxed),
            false
        );
        let block: IndexedBlock = test_data::block_h1().into();
        let _ = wrapper.verify_block(&block);
        assert_eq!(
            wrapper.enforce_full_verification.load(Ordering::Relaxed),
            true
        );
    }

    #[test]
    fn verification_level_none_accept_incorrect_block() {
        let storage: StorageRef = Arc::new(BlockChainDatabase::init_test_chain(vec![
            test_data::genesis().into(),
        ]));
        let verifier = Arc::new(ChainVerifier::new(storage.clone(), Network::Unitest));
        let bad_block: IndexedBlock = test_data::block_builder().header().build().build().into();

        // Ok(()) when nothing is verified
        let wrapper = ChainVerifierWrapper::new(
            verifier.clone(),
            &storage,
            VerificationParameters {
                verification_level: VerificationLevel::NoVerification,
                verification_edge: 1.into(),
            },
        );
        assert_eq!(wrapper.verify_block(&bad_block), Ok(()));
    }
}
