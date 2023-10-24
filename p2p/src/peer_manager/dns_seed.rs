// Copyright (c) 2021-2023 RBB S.r.l
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

use std::sync::Arc;

use async_trait::async_trait;
use common::chain::{config::ChainType, ChainConfig};
use crypto::random::{make_pseudo_rng, seq::IteratorRandom};
use logging::log;
use p2p_types::socket_address::SocketAddress;

use crate::config::P2pConfig;

#[async_trait]
pub trait DnsSeed: Send + Sync {
    async fn obtain_addresses(&self) -> Vec<SocketAddress>;
}

pub struct DefaultDnsSeed {
    chain_config: Arc<ChainConfig>,
    p2p_config: Arc<P2pConfig>,
}

impl DefaultDnsSeed {
    pub fn new(chain_config: Arc<ChainConfig>, p2p_config: Arc<P2pConfig>) -> Self {
        Self {
            chain_config,
            p2p_config,
        }
    }
}

/// Hardcoded seed DNS hostnames
// TODO: Replace with actual values
const DNS_SEEDS_MAINNET: [&str; 0] = [];
const DNS_SEEDS_TESTNET: [&str; 1] = ["testnet-seed.mintlayer.org"];

/// Maximum number of records accepted in a single DNS server response
const MAX_DNS_RECORDS: usize = 10;

#[async_trait]
impl DnsSeed for DefaultDnsSeed {
    async fn obtain_addresses(&self) -> Vec<SocketAddress> {
        let dns_seed = match self.chain_config.chain_type() {
            ChainType::Mainnet => DNS_SEEDS_MAINNET.as_slice(),
            ChainType::Testnet => DNS_SEEDS_TESTNET.as_slice(),
            ChainType::Regtest | ChainType::Signet => &[],
        };

        if dns_seed.is_empty() {
            return Vec::new();
        }

        log::debug!("Resolve DNS seed...");
        let results = futures::future::join_all(
            dns_seed
                .iter()
                .map(|host| tokio::net::lookup_host((*host, self.chain_config.p2p_port()))),
        )
        .await;

        let mut addresses = Vec::new();
        for result in results {
            match result {
                Ok(list) => {
                    list.filter_map(|addr| {
                        SocketAddress::from_peer_address(
                            // Convert SocketAddr to PeerAddress
                            &addr.into(),
                            *self.p2p_config.allow_discover_private_ips,
                        )
                    })
                    // Randomize selection because records can be sorted by type (A and AAAA)
                    .choose_multiple(&mut make_pseudo_rng(), MAX_DNS_RECORDS)
                    .into_iter()
                    .for_each(|addr| {
                        addresses.push(addr);
                    });
                }
                Err(err) => {
                    log::error!("resolve DNS seed failed: {err}");
                }
            }
        }
        log::debug!("DNS seed records found: {}", addresses.len());
        addresses
    }
}
