use super::super::rpc;
use chain::{BlockHeader, IndexedBlock};
use ecvrf;
use hex;
use miner;
use primitives::hash::H256;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use sync::{
    create_local_sync_node, create_sync_connection_factory, create_sync_peers, SyncListener,
};
use util::{init_db, node_table_path};
use {config, p2p, PROTOCOL_MINIMUM, PROTOCOL_VERSION};

enum BlockNotifierTask {
    NewBlock(H256),
    Stop,
}

struct BlockNotifier {
    tx: Sender<BlockNotifierTask>,
    is_synchronizing: Arc<AtomicBool>,
    worker_thread: Option<thread::JoinHandle<()>>,
}

impl BlockNotifier {
    pub fn new(block_notify_command: String) -> Self {
        let (tx, rx) = channel();
        let is_synchronizing = Arc::new(AtomicBool::default());
        BlockNotifier {
            tx: tx,
            is_synchronizing: is_synchronizing.clone(),
            worker_thread: Some(
                thread::Builder::new()
                    .name("Block notification thread".to_owned())
                    .spawn(move || BlockNotifier::worker(rx, block_notify_command))
                    .expect("Error creating block notification thread"),
            ),
        }
    }

    fn worker(rx: Receiver<BlockNotifierTask>, block_notify_command: String) {
        for cmd in rx {
            match cmd {
                BlockNotifierTask::NewBlock(new_block_hash) => {
                    let new_block_hash = new_block_hash.to_reversed_str();
                    let command = block_notify_command.replace("%s", &new_block_hash);
                    let c_command = ::std::ffi::CString::new(command.clone()).unwrap();
                    unsafe {
                        use libc::system;

                        let err = system(c_command.as_ptr());
                        if err != 0 {
                            error!(target: "randchaind", "Block notification command {} exited with error code {}", command, err);
                        }
                    }
                }
                BlockNotifierTask::Stop => break,
            }
        }
        trace!(target: "randchaind", "Block notification thread stopped");
    }
}

impl SyncListener for BlockNotifier {
    fn synchronization_state_switched(&self, is_synchronizing: bool) {
        self.is_synchronizing
            .store(is_synchronizing, Ordering::SeqCst);
    }

    fn best_storage_block_inserted(&self, block_hash: &H256) {
        if !self.is_synchronizing.load(Ordering::SeqCst) {
            self.tx
                .send(BlockNotifierTask::NewBlock(block_hash.clone()))
                .expect("Block notification thread have the same lifetime as `BlockNotifier`")
        }
    }
}

impl Drop for BlockNotifier {
    fn drop(&mut self) {
        if let Some(join_handle) = self.worker_thread.take() {
            let _ = self.tx.send(BlockNotifierTask::Stop);
            join_handle.join().expect("Clean shutdown.");
        }
    }
}

pub fn start(cfg: config::Config) -> Result<(), String> {
    let mut el = p2p::event_loop();

    init_db(&cfg)?;

    let nodes_path = node_table_path(&cfg);

    let p2p_cfg = p2p::Config {
        threads: cfg.p2p_threads,
        inbound_connections: cfg.inbound_connections,
        outbound_connections: cfg.outbound_connections,
        connection: p2p::NetConfig {
            protocol_version: PROTOCOL_VERSION,
            protocol_minimum: PROTOCOL_MINIMUM,
            magic: cfg.network.magic(),
            local_address: SocketAddr::new(cfg.host, cfg.port),
            services: cfg.services,
            user_agent: cfg.user_agent,
            start_height: 0,
            relay: true,
            network: cfg.network,
        },
        peers: cfg.peers,
        seeds: cfg.seednodes,
        node_table_path: nodes_path,
        preferable_services: cfg.services,
        internet_protocol: cfg.internet_protocol,
    };

    let sync_peers = create_sync_peers();
    let local_sync_node = create_local_sync_node(
        cfg.network,
        cfg.db.clone(),
        sync_peers.clone(),
        cfg.verification_params,
    );
    let sync_connection_factory =
        create_sync_connection_factory(sync_peers.clone(), local_sync_node.clone());

    if let Some(block_notify_command) = cfg.block_notify_command {
        local_sync_node.install_sync_listener(Box::new(BlockNotifier::new(block_notify_command)));
    }

    // start P2P server
    let p2p =
        p2p::P2P::new(p2p_cfg, sync_connection_factory, el.handle()).map_err(|x| x.to_string())?;
    p2p.run().map_err(|_| "Failed to start p2p module")?;

    // start RPC server
    let rpc_deps = rpc::Dependencies {
        network: cfg.network,
        storage: cfg.db.clone(),
        local_sync_node: local_sync_node.clone(),
        p2p_context: p2p.context().clone(),
        remote: el.remote(),
    };
    let _rpc_server = rpc::new_http(cfg.rpc_config, rpc_deps)?;

    // Miner
    let network_target: u32 = (cfg.num_miners * cfg.num_nodes * cfg.blocktime).into();
    for _ in 0..cfg.num_miners {
        let (_, pk) = ecvrf::keygen();
        let local_sync_node = local_sync_node.clone();
        thread::spawn(move || {
            let mut iters = 0;
            loop {
                let blktpl = local_sync_node.get_block_template();
                if let Some(solution) =
                    miner::mock::try_solve_one_shot(&blktpl, &pk, iters, network_target)
                {
                    let blk = chain::Block {
                        block_header: BlockHeader {
                            version: blktpl.version,
                            previous_header_hash: blktpl.previous_header_hash,
                            time: blktpl.time,
                            bits: blktpl.bits,
                            pubkey: pk.clone(),
                            iterations: solution.iterations,
                            randomness: solution.randomness,
                        },
                        proof: solution.proof,
                    };
                    trace!(
                        "Block {} mined by {}!",
                        blk.hash().to_reversed_str(),
                        hex::encode(pk.to_bytes())
                    );
                    // Let's use PeerIndex=0 to identify the node itself
                    local_sync_node.on_block(0, IndexedBlock::from(blk));
                    iters = 0;
                }
            }
        });
    }

    // Keep the main process running forever
    el.run(p2p::forever()).unwrap();
    Ok(())
}
