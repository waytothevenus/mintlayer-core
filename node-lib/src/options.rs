// Copyright (c) 2022 RBB S.r.l
// opensource@mintlayer.org
// SPDX-License-Identifier: MIT
// Licensed under the MIT License;
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// https://github.com/mintlayer/mintlayer-core/blob/master/LICENSE
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The node command line options.

use std::{
    ffi::OsString,
    net::{IpAddr, SocketAddr},
    num::NonZeroU64,
    path::PathBuf,
};

use clap::{Args, Parser, Subcommand};
use common::chain::config::{regtest_options::ChainConfigOptions, ChainType};
use p2p::types::ip_or_socket_address::IpOrSocketAddress;
use utils::default_data_dir::default_data_dir_common;

use crate::config_files::{NodeTypeConfigFile, StorageBackendConfigFile};

const CONFIG_NAME: &str = "config.toml";

/// Mintlayer node executable
#[derive(Parser, Debug)]
#[clap(author, version, about)]
pub struct Options {
    /// The path to the data directory.
    #[clap(short, long = "datadir", env = "MINTLAYER_DATADIR")]
    pub data_dir: Option<PathBuf>,

    #[clap(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Clone, Debug)]
pub enum Command {
    /// Run the mainnet node.
    Mainnet(RunOptions),
    /// Run the testnet node.
    Testnet(RunOptions),
    /// Run the regtest node.
    Regtest(Box<RegtestOptions>),
}

#[derive(Args, Clone, Debug)]
pub struct RegtestOptions {
    #[clap(flatten)]
    pub run_options: RunOptions,
    #[clap(flatten)]
    pub chain_config: ChainConfigOptions,
}

#[derive(Args, Clone, Debug, Default)]
pub struct RunOptions {
    /// A flag that will clean the data dir before starting
    #[clap(long, short, action = clap::ArgAction::SetTrue, env = "MINTLAYER_CLEAN_DATA")]
    pub clean_data: Option<bool>,

    /// Minimum number of connected peers to enable block production.
    #[clap(
        long,
        value_name = "COUNT",
        env = "MINTLAYER_BLOCKPROD_MIN_PEERS_TO_PRODUCE_BLOCKS"
    )]
    pub blockprod_min_peers_to_produce_blocks: Option<usize>,

    /// Skip the initial block download check for block production.
    ///
    /// When a node starts, it checks if it has the latest block. If
    /// not, it downloads the missing blocks from its peers. This is
    /// called the initial block download (IBD).
    ///
    /// If this flag is set, the node will skip the IBD
    /// check and start producing blocks immediately. This option
    /// should only be used once, when the node is starting from
    /// Genesis. If used on a node that is not starting from Genesis,
    /// the node may produce blocks from the past, which will lead
    /// to being banned by the network.
    #[clap(long, action = clap::ArgAction::SetTrue, env = "MINTLAYER_BLOCKPROD_SKIP_IBD_CHECK")]
    pub blockprod_skip_ibd_check: Option<bool>,

    /// Storage backend to use.
    #[clap(long, env = "MINTLAYER_STORAGE_BACKEND")]
    pub storage_backend: Option<StorageBackendConfigFile>,

    /// The node type.
    #[clap(long, env = "MINTLAYER_NODE_TYPE")]
    pub node_type: Option<NodeTypeConfigFile>,

    /// Mock time used to initialize the node time at startup, in seconds (valid only for regtest).
    #[clap(long)]
    #[arg(hide = true, env = "MINTLAYER_MOCK_TIME")]
    pub mock_time: Option<u64>,

    /// The number of maximum attempts to process a block.
    #[clap(long, value_name = "COUNT", env = "MINTLAYER_MAX_DB_COMMIT_ATTEMPTS")]
    pub max_db_commit_attempts: Option<usize>,

    /// The maximum capacity of the orphan blocks pool in blocks.
    #[clap(long, value_name = "COUNT", env = "MINTLAYER_MAX_ORPHAN_BLOCKS")]
    pub max_orphan_blocks: Option<usize>,

    /// Addresses to bind P2P to.
    /// Can be specified multiple times and/or be a comma-separated list.
    #[clap(
        long,
        value_name = "ADDR",
        value_delimiter(','),
        env = "MINTLAYER_P2P_BIND_ADDRESSES"
    )]
    pub p2p_bind_addresses: Option<Vec<String>>,

    /// Connect through SOCKS5 proxy.
    #[clap(long, value_name = "PROXY", env = "MINTLAYER_P2P_SOCKS5_PROXY")]
    pub p2p_socks5_proxy: Option<String>,

    /// Disable p2p encryption (for tests only).
    #[clap(long, action = clap::ArgAction::SetTrue, env = "MINTLAYER_P2P_DISABLE_NOISE")]
    #[arg(hide = true)]
    pub p2p_disable_noise: Option<bool>,

    /// Optional list of boot node addresses to connect.
    /// Can be specified multiple times and/or be a comma-separated list.
    #[clap(
        long,
        value_name = "ADDR",
        value_delimiter(','),
        env = "MINTLAYER_P2P_BOOT_NODES"
    )]
    pub p2p_boot_nodes: Option<Vec<IpOrSocketAddress>>,

    /// Optional list of reserved node addresses to connect.
    /// Can be specified multiple times and/or be a comma-separated list.
    #[clap(
        long,
        value_name = "ADDR",
        value_delimiter(','),
        env = "MINTLAYER_P2P_RESERVED_NODES"
    )]
    pub p2p_reserved_nodes: Option<Vec<IpOrSocketAddress>>,

    /// Optional list of whitelisted addresses.
    /// Can be specified multiple times and/or be a comma-separated list.
    #[clap(
        long,
        value_name = "ADDR",
        value_delimiter(','),
        env = "MINTLAYER_P2P_WHITELIST_ADDR"
    )]
    pub p2p_whitelist_addr: Option<Vec<IpAddr>>,

    /// Maximum allowed number of inbound connections.
    #[clap(
        long,
        value_name = "COUNT",
        env = "MINTLAYER_P2P_MAX_INBOUND_CONNECTIONS"
    )]
    pub p2p_max_inbound_connections: Option<usize>,

    /// The p2p score threshold after which a peer is discouraged.
    #[clap(
        long,
        value_name = "THRESHOLD",
        env = "MINTLAYER_P2P_DISCOURAGEMENT_THRESHOLD"
    )]
    pub p2p_discouragement_threshold: Option<u32>,

    /// The p2p discouragement duration in seconds.
    #[clap(
        long,
        value_name = "DURATION",
        env = "MINTLAYER_P2P_DISCOURAGEMENT_DURATION"
    )]
    pub p2p_discouragement_duration: Option<u64>,

    /// The p2p timeout value in seconds.
    #[clap(
        long,
        value_name = "TIMEOUT",
        env = "MINTLAYER_P2P_OUTBOUND_CONNECTION_TIMEOUT"
    )]
    pub p2p_outbound_connection_timeout: Option<NonZeroU64>,

    /// How often send ping requests to peers (in seconds).
    /// Set to 0 to disable sending ping requests.
    #[clap(long, value_name = "PERIOD", env = "MINTLAYER_P2P_PING_CHECK_PERIOD")]
    pub p2p_ping_check_period: Option<u64>,

    /// After what time a peer is detected as dead and is disconnected (in seconds).
    #[clap(long, value_name = "TIMEOUT", env = "MINTLAYER_P2P_PING_TIMEOUT")]
    pub p2p_ping_timeout: Option<NonZeroU64>,

    /// A timeout after which a peer is disconnected.
    #[clap(
        long,
        value_name = "TIMEOUT",
        env = "MINTLAYER_P2P_SYNC_STALLING_TIMEOUT"
    )]
    pub p2p_sync_stalling_timeout: Option<NonZeroU64>,

    /// Maximum acceptable time difference between this node and the remote peer (in seconds).
    /// If a large difference is detected, the peer will be disconnected.
    #[clap(long, value_name = "DIFF", env = "MINTLAYER_P2P_MAX_CLOCK_DIFF")]
    pub p2p_max_clock_diff: Option<u64>,

    /// If true, the node will perform an early dns query if the peer db doesn't contain
    /// any global addresses at startup.
    #[clap(long, action = clap::ArgAction::SetTrue, env = "MINTLAYER_P2P_FORCE_DNS_QUERY_IF_NO_GLOBAL_ADDRESSES_KNOWN")]
    #[arg(hide = true)]
    pub p2p_force_dns_query_if_no_global_addresses_known: Option<bool>,

    /// A maximum tip age in seconds.
    ///
    /// The initial block download is finished if the difference between the current time and the
    /// tip time is less than this value.
    #[clap(long, value_name = "AGE", env = "MINTLAYER_MAX_TIP_AGE")]
    pub max_tip_age: Option<u64>,

    /// Address to bind RPC to.
    #[clap(long, value_name = "ADDR", env = "MINTLAYER_RPC_BIND_ADDRESS")]
    pub rpc_bind_address: Option<SocketAddr>,

    /// Enable/Disable http RPC.
    #[clap(long, value_name = "VAL", env = "MINTLAYER_RPC_ENABLED")]
    pub rpc_enabled: Option<bool>,

    /// Username for RPC server basic authorization.
    /// If not set, the cookie file is created.
    #[clap(long, value_name = "USERNAME", env = "MINTLAYER_RPC_USERNAME")]
    pub rpc_username: Option<String>,

    /// Password for RPC server basic authorization.
    /// If not set, the RPC cookie file is created.
    #[clap(long, value_name = "PASSWORD", env = "MINTLAYER_RPC_PASSWORD")]
    pub rpc_password: Option<String>,

    /// Custom file path for the RPC cookie file.
    /// If not set, the cookie file is created in the data dir.
    #[clap(long, value_name = "PATH", env = "MINTLAYER_RPC_COOKIE_FILE")]
    pub rpc_cookie_file: Option<String>,

    /// Minimum transaction relay fee rate (in atoms per 1000 bytes).
    #[clap(long, value_name = "VAL", env = "MINTLAYER_MIN_TX_RELAY_FEE_RATE")]
    pub min_tx_relay_fee_rate: Option<u64>,
}

impl Options {
    /// Constructs an instance by parsing the given arguments.
    ///
    /// The data directory is created as a side-effect of the invocation.
    /// Process is terminated on error.
    pub fn from_args<A: Into<OsString> + Clone>(args: impl IntoIterator<Item = A>) -> Self {
        Parser::parse_from(args)
    }

    /// Returns the data directory
    pub fn data_dir(&self) -> &Option<PathBuf> {
        &self.data_dir
    }

    /// Returns a path to the config file
    pub fn config_path(&self, chain_type: ChainType) -> PathBuf {
        self.data_dir
            .clone()
            .unwrap_or_else(|| default_data_dir(chain_type))
            .join(CONFIG_NAME)
    }
}

pub fn default_data_dir(chain_type: ChainType) -> PathBuf {
    default_data_dir_common().join(chain_type.name())
}
