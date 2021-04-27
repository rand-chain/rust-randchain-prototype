use clap;
use message::Services;
use network::Network;
use p2p::InternetProtocol;
use primitives::hash::H256;
use rpc::HttpConfiguration as RpcHttpConfig;
use rpc_apis::ApiSet;
use seednodes::{mainnet_seednodes, testnet_seednodes};
use std::fs;
use std::net;
use storage;
use sync::VerificationParameters;
use util::open_db;
use verification::VerificationLevel;
use {REGTEST_USER_AGENT, USER_AGENT};

pub struct Config {
    pub network: Network,
    pub services: Services,
    pub port: u16,
    pub peers: Vec<net::SocketAddr>,
    pub host: net::IpAddr,
    pub seednodes: Vec<String>, // we use String rather than SocketAddr as DNS resolver takes String
    pub quiet: bool,
    pub inbound_connections: u32,
    pub outbound_connections: u32,
    pub p2p_threads: usize,
    pub db_cache: usize,
    pub data_dir: Option<String>,
    pub user_agent: String,
    pub internet_protocol: InternetProtocol,
    pub rpc_config: RpcHttpConfig,
    pub block_notify_command: Option<String>,
    pub verification_params: VerificationParameters,
    pub db: storage::SharedStore,
    pub num_nodes: u16,
    pub num_miners: u16,
    pub blocktime: u16,
}

pub const DEFAULT_DB_CACHE: usize = 512;

pub fn parse(matches: &clap::ArgMatches) -> Result<Config, String> {
    let db_cache = match matches.value_of("db-cache") {
        Some(s) => s
            .parse()
            .map_err(|_| "Invalid cache size - should be number in MB".to_owned())?,
        None => DEFAULT_DB_CACHE,
    };

    let data_dir = match matches.value_of("data-dir") {
        Some(s) => Some(s.parse().map_err(|_| "Invalid data-dir".to_owned())?),
        None => None,
    };

    let db = open_db(&data_dir, db_cache);

    let quiet = matches.is_present("quiet");
    let network = match (matches.is_present("testnet"), matches.is_present("regtest")) {
        (true, false) => Network::Testnet,
        (false, true) => Network::Regtest,
        (false, false) => Network::Mainnet,
        (true, true) => return Err("Only one testnet option can be used".into()),
    };

    let (in_connections, out_connections) = match network {
        Network::Testnet | Network::Mainnet | Network::Other(_) => (125, 8),
        Network::Regtest | Network::Unitest => (1, 0),
    };

    let p2p_threads = match network {
        Network::Testnet | Network::Mainnet | Network::Other(_) => 4,
        Network::Regtest | Network::Unitest => 1,
    };

    let user_agent = match network {
        Network::Testnet | Network::Mainnet | Network::Unitest | Network::Other(_) => {
            USER_AGENT.into()
        }
        Network::Regtest => REGTEST_USER_AGENT.into(),
    };

    let port = match matches.value_of("port") {
        Some(port) => port.parse().map_err(|_| "Invalid port".to_owned())?,
        None => network.port(),
    };

    // construct nodes needed to be connected
    // both --peers and --peers-file can be used for specifying peers
    // if <peers-file> is given, <peers> will be ignored
    let mut peers = match matches.value_of("peers") {
        Some(addrs_cfg) => {
            let mut addrs: Vec<net::SocketAddr> = vec![];
            for addr_str in addrs_cfg.split(",") {
                match addr_str.parse::<net::SocketAddr>() {
                    Err(_) => {
                        // no port given, enforce the default port
                        let addr = addr_str
                            .parse::<net::IpAddr>()
                            .map(|ip| net::SocketAddr::new(ip, network.port()))
                            .unwrap();
                        addrs.push(addr);
                    }
                    Ok(a) => addrs.push(a), // with port given
                }
            }
            addrs
        }
        None => vec![],
    };
    if let Some(peers_file_path) = matches.value_of("peers-file") {
        peers = vec![];
        let addrs_cfg: String =
            fs::read_to_string(peers_file_path).expect("Something went wrong reading peers-file");
        for addr_str in addrs_cfg.split('\n') {
            match addr_str.parse::<net::SocketAddr>() {
                Err(_) => {
                    // no port given, enforce the default port
                    let addr = addr_str
                        .parse::<net::IpAddr>()
                        .map(|ip| net::SocketAddr::new(ip, network.port()))
                        .unwrap();
                    peers.push(addr);
                }
                Ok(a) => peers.push(a), // with port given
            }
        }
    }

    let seednodes: Vec<String> = match matches.value_of("seednode") {
        Some(addrs_cfg) => {
            let mut addrs: Vec<String> = vec![];
            for addr_str in addrs_cfg.split(",") {
                match addr_str.parse::<net::SocketAddr>() {
                    Err(_) => addrs.push(format!("{}:{}", addr_str, network.dns_port())), // no port given, enforce the default port
                    Ok(_) => addrs.push(addr_str.to_owned()), // with port given
                }
            }
            addrs
        }
        None => match network {
            Network::Mainnet => mainnet_seednodes().into_iter().map(Into::into).collect(),
            Network::Testnet => testnet_seednodes().into_iter().map(Into::into).collect(),
            Network::Other(_) | Network::Regtest | Network::Unitest => Vec::new(),
        },
    };

    let only_net = match matches.value_of("only-net") {
        Some(s) => s.parse()?,
        None => InternetProtocol::default(),
    };

    let host = match matches.value_of("host") {
        Some(s) => s
            .parse::<net::IpAddr>()
            .map_err(|_| "Invalid host".to_owned())?,
        None => match only_net {
            InternetProtocol::IpV6 => "::".parse().unwrap(),
            _ => "0.0.0.0".parse().unwrap(),
        },
    };

    let rpc_config = parse_rpc_config(network, matches)?;

    let block_notify_command = match matches.value_of("blocknotify") {
        Some(s) => Some(
            s.parse()
                .map_err(|_| "Invalid blocknotify commmand".to_owned())?,
        ),
        None => None,
    };

    let services = Services::default().with_network(true);

    let verification_level = match matches.value_of("verification-level") {
        Some(s) if s == "full" => VerificationLevel::Full,
        Some(s) if s == "header" => VerificationLevel::Header,
        Some(s) if s == "none" => VerificationLevel::NoVerification,
        Some(s) => return Err(format!("Invalid verification level: {}", s)),
        None => VerificationLevel::Full,
    };

    // TODO:
    let verification_edge = match matches.value_of("verification-edge") {
        Some(s) if verification_level != VerificationLevel::Full => {
            let edge: H256 = s
                .parse()
                .map_err(|_| "Invalid verification edge".to_owned())?;
            edge.reversed()
        }
        _ => network.default_verification_edge(),
    };

    let num_nodes = match matches.value_of("num-nodes") {
        Some(s) => s.parse().map_err(|_| "Invalid num_nodes".to_owned())?,
        None => 4,
    };

    let num_miners = match matches.value_of("num-miners") {
        Some(s) => s.parse().map_err(|_| "Invalid num_miners".to_owned())?,
        None => 1,
    };

    let blocktime = match matches.value_of("blocktime") {
        Some(s) => s.parse().map_err(|_| "Invalid blocktime".to_owned())?,
        None => 10, // 10s
    };

    let config = Config {
        quiet: quiet,
        network: network,
        services: services,
        port: port,
        peers: peers,
        host: host,
        seednodes: seednodes,
        inbound_connections: in_connections,
        outbound_connections: out_connections,
        p2p_threads: p2p_threads,
        db_cache: db_cache,
        data_dir: data_dir,
        user_agent: user_agent,
        internet_protocol: only_net,
        rpc_config: rpc_config,
        block_notify_command: block_notify_command,
        verification_params: VerificationParameters {
            verification_level: verification_level,
            verification_edge: verification_edge,
        },
        db: db,
        num_nodes: num_nodes,
        num_miners: num_miners,
        blocktime: blocktime,
    };

    Ok(config)
}

fn parse_rpc_config(network: Network, matches: &clap::ArgMatches) -> Result<RpcHttpConfig, String> {
    let mut config = RpcHttpConfig::with_port(network.rpc_port());
    config.enabled = !matches.is_present("no-jsonrpc");
    if !config.enabled {
        return Ok(config);
    }

    if let Some(apis) = matches.value_of("jsonrpc-apis") {
        config.apis = ApiSet::List(
            vec![apis.parse().map_err(|_| "Invalid APIs".to_owned())?]
                .into_iter()
                .collect(),
        );
    }
    if let Some(port) = matches.value_of("jsonrpc-port") {
        config.port = port
            .parse()
            .map_err(|_| "Invalid JSON RPC port".to_owned())?;
    }
    if let Some(interface) = matches.value_of("jsonrpc-interface") {
        config.interface = interface.to_owned();
    }
    if let Some(cors) = matches.value_of("jsonrpc-cors") {
        config.cors = Some(vec![cors
            .parse()
            .map_err(|_| "Invalid JSON RPC CORS".to_owned())?]);
    }
    if let Some(hosts) = matches.value_of("jsonrpc-hosts") {
        config.hosts = Some(vec![hosts
            .parse()
            .map_err(|_| "Invalid JSON RPC hosts".to_owned())?]);
    }

    Ok(config)
}
