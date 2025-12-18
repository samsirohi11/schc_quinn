use clap::{Parser, Subcommand};
use std::net::IpAddr;
use std::path::PathBuf;

#[derive(Parser, Debug, Clone)]
pub struct CliOpt {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Run the QUIC simulation
    Quic(QuicOpt),
    /// Run a ping simulation at the UDP level
    Ping(PingOpt),
    /// Run a throughput simulation at the UDP level
    Throughput(ThroughputOpt),
    /// Return the identifier of the async runtime used
    Rt,
}

#[derive(Parser, Debug, Clone)]
pub struct NetworkOpt {
    /// The IP address of the node used as a client
    #[arg(long)]
    pub client_ip_address: IpAddr,

    /// The IP address of the node used as a server
    #[arg(long)]
    pub server_ip_address: IpAddr,

    /// Whether the run should be non-deterministic, i.e. using a non-constant seed for the random
    /// number generators
    #[arg(long)]
    pub non_deterministic: bool,

    /// Quinn's random seed, which you can control to generate deterministic results (Quinn uses
    /// randomness internally)
    #[arg(long, default_value_t = 0)]
    pub quinn_rng_seed: u64,

    /// The random seed used for the simulated network (governing packet loss, duplication and
    /// reordering)
    #[arg(long, default_value_t = 42)]
    pub network_rng_seed: u64,

    /// Path to the JSON file containing the network graph
    #[arg(long)]
    pub network_graph: PathBuf,

    /// Path to the JSON file containing the network events
    #[arg(long)]
    pub network_events: PathBuf,
}

#[derive(Parser, Debug, Clone)]
pub struct QuicOpt {
    /// The number of requests that should be made
    #[arg(long, default_value_t = 10)]
    pub requests: u32,

    /// The number of concurrent connections used when making the requests
    #[arg(long, default_value_t = 1)]
    pub concurrent_connections: u8,

    /// The number of concurrent streams per connection used when making the requests
    #[arg(long, default_value_t = 1)]
    pub concurrent_streams_per_connection: u32,

    /// The size of each response, in bytes
    #[arg(long, default_value_t = 1024)]
    pub response_size: usize,

    /// Enable SCHC observer mode (logs compression without modifying packets)
    #[arg(long, default_value_t = false)]
    pub schc_observer: bool,

    /// Path to SCHC rules JSON file
    #[arg(long)]
    pub schc_rules: Option<PathBuf>,

    /// Node IDs where SCHC observer should be active (comma-separated, e.g., "MoonOrbiter1")
    /// If not specified, SCHC is applied at all router nodes
    #[arg(long, value_delimiter = ',')]
    pub schc_nodes: Option<Vec<String>>,

    /// Enable verbose SCHC debug output showing per-packet matching and compression details
    #[arg(long, default_value_t = false)]
    pub schc_debug: bool,

    /// Enable SCHC compression mode (actually compress/decompress packets)
    #[arg(long, default_value_t = false)]
    pub schc_compress: bool,

    /// Node IDs where SCHC compression is active (comma-separated, e.g., "SchcNode1,SchcNode2")
    /// These nodes will compress packets going UP and decompress packets going DOWN
    #[arg(long, value_delimiter = ',')]
    pub schc_compress_nodes: Option<Vec<String>>,

    /// Enable dynamic QUIC rule generation based on learned connection IDs
    /// When enabled, SCHC learns DCIDs/SCIDs from handshake packets and generates
    /// more specific rules for better compression of subsequent packets
    #[arg(long, default_value_t = false)]
    pub schc_dynamic_quic_rules: bool,

    #[command(flatten)]
    pub network: NetworkOpt,
}

#[derive(Parser, Debug, Clone)]
pub struct PingOpt {
    /// The duration of the run, after which we will stop sending pings and the program will
    /// terminate
    #[arg(long)]
    pub duration_ms: u64,

    /// The interval at which ping packets will be sent
    #[arg(long)]
    pub interval_ms: u64,

    /// The deadline between sending a ping and receiving a reply (after which the ping itself or
    /// its reply are considered lost)
    #[arg(long, default_value_t = 10_000)]
    pub deadline_ms: u64,

    #[command(flatten)]
    pub network: NetworkOpt,
}

#[derive(Parser, Debug, Clone)]
pub struct ThroughputOpt {
    /// The duration of the run
    #[arg(long)]
    pub duration_ms: u64,

    /// The bitrate at which information should be sent
    ///
    /// If not provided, we find the link with the highest capacity and use its doubled bandwidth
    #[arg(long)]
    pub send_bps: Option<u64>,

    #[command(flatten)]
    pub network: NetworkOpt,
}
