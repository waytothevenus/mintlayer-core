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

use std::{sync::Arc, time::Duration};

use libp2p::multiaddr::Protocol;

use p2p::testing_utils::{TestTransportLibp2p, TestTransportMaker};
use p2p::{
    config::{MdnsConfig, NodeType, P2pConfig},
    net::{
        libp2p::Libp2pService, types::ConnectivityEvent, ConnectivityService, NetworkingService,
    },
};

// verify that libp2p mdns peer discovery works
#[tokio::test]
async fn test_libp2p_peer_discovery() {
    let config = Arc::new(common::chain::config::create_mainnet());
    let (mut serv, _) = Libp2pService::start(
        TestTransportLibp2p::make_transport(),
        TestTransportLibp2p::make_address(),
        Arc::clone(&config),
        Arc::new(P2pConfig {
            bind_address: "/ip6/::1/tcp/3031".to_owned().into(),
            ban_threshold: 100.into(),
            outbound_connection_timeout: 10.into(),
            mdns_config: MdnsConfig::Enabled {
                query_interval: 200.into(),
                enable_ipv6_mdns_discovery: Default::default(),
            }
            .into(),
            request_timeout: Duration::from_secs(10).into(),
            node_type: NodeType::Full.into(),
        }),
    )
    .await
    .unwrap();

    let (mut serv2, _) = Libp2pService::start(
        TestTransportLibp2p::make_transport(),
        TestTransportLibp2p::make_address(),
        Arc::clone(&config),
        Arc::new(P2pConfig {
            bind_address: "/ip6/::1/tcp/3031".to_owned().into(),
            ban_threshold: 100.into(),
            outbound_connection_timeout: 10.into(),
            mdns_config: MdnsConfig::Enabled {
                query_interval: 200.into(),
                enable_ipv6_mdns_discovery: Default::default(),
            }
            .into(),
            request_timeout: Duration::from_secs(10).into(),
            node_type: NodeType::Full.into(),
        }),
    )
    .await
    .unwrap();

    loop {
        let (serv_res, _) = tokio::join!(serv.poll_next(), serv2.poll_next());

        match serv_res.unwrap() {
            ConnectivityEvent::Discovered { peers } => {
                assert!(!peers.is_empty());

                // verify that all discovered addresses are either ipv4 or ipv6,
                // they have tcp as the transport protocol and that all end with the peer id
                for peer in peers {
                    for addr in peer.ip6.iter().chain(peer.ip4.iter()) {
                        let mut components = addr.iter();
                        assert!(matches!(
                            components.next(),
                            Some(Protocol::Ip6(_) | Protocol::Ip4(_))
                        ));
                        assert!(matches!(components.next(), Some(Protocol::Tcp(_))));
                        assert!(matches!(components.next(), Some(Protocol::P2p(_))));
                    }
                }

                return;
            }
            e => panic!("unexpected event: {:?}", e),
        }
    }
}
