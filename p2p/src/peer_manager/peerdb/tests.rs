// Copyright (c) 2022-2023 RBB S.r.l
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

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    sync::Arc,
    time::Duration,
};

use itertools::Itertools;
use rstest::rstest;

use ::test_utils::random::{make_seedable_rng, Seed};
use common::{
    chain::config::create_unit_test_config, primitives::user_agent::mintlayer_core_user_agent,
};
use crypto::random::Rng;
use p2p_test_utils::P2pBasicTestTimeGetter;

use crate::{
    config::P2pConfig,
    peer_manager::{
        peerdb::{
            address_data::{PURGE_REACHABLE_FAIL_COUNT, PURGE_UNREACHABLE_TIME},
            address_tables::RandomKey,
            storage::{KnownAddressState, PeerDbStorageRead},
        },
        peerdb_common::Transactional,
    },
    testing_utils::{peerdb_inmemory_store, test_p2p_config, TestAddressMaker},
};

use super::{
    address_tables::{
        table::Table,
        test_utils::{make_non_colliding_addresses, make_random_address},
    },
    config::PeerDbConfig,
    storage::PeerDbStorage,
    PeerDb,
};

#[tracing::instrument]
#[test]
fn unban_peer() {
    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let chain_config = create_unit_test_config();
    let mut peerdb = PeerDb::<_>::new(
        &chain_config,
        Arc::new(P2pConfig {
            ban_duration: Duration::from_secs(60).into(),

            bind_addresses: Default::default(),
            socks5_proxy: None,
            disable_noise: Default::default(),
            boot_nodes: Default::default(),
            reserved_nodes: Default::default(),
            ban_threshold: Default::default(),
            outbound_connection_timeout: Default::default(),
            ping_check_period: Default::default(),
            ping_timeout: Default::default(),
            peer_handshake_timeout: Default::default(),
            max_clock_diff: Default::default(),
            node_type: Default::default(),
            allow_discover_private_ips: Default::default(),
            user_agent: mintlayer_core_user_agent(),
            sync_stalling_timeout: Default::default(),
            peer_manager_config: Default::default(),
            protocol_config: Default::default(),
        }),
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    let address = TestAddressMaker::new_random_address();
    peerdb.ban(address.as_bannable());

    assert!(peerdb.is_address_banned(&address.as_bannable()));
    let banned_addresses = peerdb.storage.transaction_ro().unwrap().get_banned_addresses().unwrap();
    assert_eq!(banned_addresses.len(), 1);

    time_getter.advance_time(Duration::from_secs(120));

    // Banned addresses updated in the `heartbeat` function
    peerdb.heartbeat();

    assert!(!peerdb.is_address_banned(&address.as_bannable()));
    let banned_addresses = peerdb.storage.transaction_ro().unwrap().get_banned_addresses().unwrap();
    assert_eq!(banned_addresses.len(), 0);

    assert_addr_consistency(&peerdb);
}

#[tracing::instrument]
#[test]
fn connected_unreachable() {
    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let p2p_config = Arc::new(test_p2p_config());
    let chain_config = create_unit_test_config();
    let mut peerdb = PeerDb::new(
        &chain_config,
        p2p_config,
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    let address = TestAddressMaker::new_random_address();
    peerdb.peer_discovered(address);
    peerdb.report_outbound_failure(address);
    assert!(peerdb.addresses.get(&address).unwrap().is_unreachable());

    // User requests connection to the currently unreachable node via RPC and connection succeeds.
    // PeerDb should process that normally.
    peerdb.outbound_peer_connected(address);
    assert!(peerdb.addresses.get(&address).unwrap().is_connected());

    assert_addr_consistency(&peerdb);
}

#[tracing::instrument]
#[test]
fn connected_unknown() {
    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let chain_config = create_unit_test_config();
    let p2p_config = Arc::new(test_p2p_config());
    let mut peerdb = PeerDb::new(
        &chain_config,
        p2p_config,
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    let address = TestAddressMaker::new_random_address();

    // User requests connection to some unknown node via RPC and connection succeeds.
    // PeerDb should process that normally.
    peerdb.outbound_peer_connected(address);
    assert!(peerdb.addresses.get(&address).unwrap().is_connected());

    assert_addr_consistency(&peerdb);
}

#[tracing::instrument]
#[test]
fn anchor_peers() {
    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let chain_config = create_unit_test_config();
    let p2p_config = Arc::new(test_p2p_config());

    let mut peerdb = PeerDb::new(
        &chain_config,
        Arc::clone(&p2p_config),
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    let mut anchors =
        [TestAddressMaker::new_random_address(), TestAddressMaker::new_random_address()]
            .into_iter()
            .collect::<BTreeSet<_>>();

    peerdb.set_anchors(anchors.clone());
    assert_eq!(*peerdb.anchors(), anchors);

    let new_address = TestAddressMaker::new_random_address();
    anchors.insert(new_address);
    peerdb.set_anchors(anchors.clone());
    assert_eq!(*peerdb.anchors(), anchors);

    let mut peerdb = PeerDb::new(
        &chain_config,
        Arc::clone(&p2p_config),
        time_getter.get_time_getter(),
        peerdb.storage,
    )
    .unwrap();
    assert_eq!(*peerdb.anchors(), anchors);

    anchors.remove(&new_address);
    peerdb.set_anchors(anchors.clone());
    assert_eq!(*peerdb.anchors(), anchors);
    let peerdb = PeerDb::new(
        &chain_config,
        Arc::clone(&p2p_config),
        time_getter.get_time_getter(),
        peerdb.storage,
    )
    .unwrap();
    assert_eq!(*peerdb.anchors(), anchors);

    assert_addr_consistency(&peerdb);
}

// Call 'remove_outbound_address' on new and tried addresses, check that the db is
// in consistent state.
#[tracing::instrument(skip(seed))]
#[rstest]
#[trace]
#[case(Seed::from_entropy())]
fn remove_addr(#[case] seed: Seed) {
    let mut rng = make_seedable_rng(seed);

    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let chain_config = create_unit_test_config();
    let p2p_config = Arc::new(test_p2p_config());
    let peerdb_config = PeerDbConfig {
        addr_tables_bucket_size: 10.into(),
        new_addr_table_bucket_count: 10.into(),
        tried_addr_table_bucket_count: 10.into(),
        addr_tables_initial_random_key: Some(RandomKey::new_random_with_rng(&mut rng)),
    };

    let mut peerdb = PeerDb::new_with_config(
        &chain_config,
        Arc::clone(&p2p_config),
        &peerdb_config,
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    let addr_count = 10;

    let new_addrs = make_non_colliding_addresses(new_addr_table(&peerdb), addr_count, &mut rng);
    let tried_addrs = make_non_colliding_addresses(tried_addr_table(&peerdb), addr_count, &mut rng);

    let (new_addrs_to_remove, new_addrs_to_keep) = split_in_two_sets(&new_addrs, &mut rng);
    let (tried_addrs_to_remove, tried_addrs_to_keep) = split_in_two_sets(&tried_addrs, &mut rng);

    // Reserved addresses are often treated differently, so mark two of the to-remove addresses
    // as reserved.
    peerdb.add_reserved_node(*new_addrs_to_remove.first().unwrap());
    peerdb.add_reserved_node(*tried_addrs_to_remove.first().unwrap());

    for addr in &new_addrs {
        peerdb.peer_discovered(*addr);
    }

    for addr in &tried_addrs {
        peerdb.outbound_peer_connected(*addr);
    }

    for addr in new_addrs_to_remove.iter().chain(tried_addrs_to_remove.iter()) {
        peerdb.remove_outbound_address(addr);
    }

    let new_addrs_remaining = new_addr_table(&peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    let tried_addrs_remaining =
        tried_addr_table(&peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    assert_eq_sets(new_addrs_remaining.iter(), new_addrs_to_keep.iter());
    assert_eq_sets(tried_addrs_remaining.iter(), tried_addrs_to_keep.iter());
    assert_addr_consistency(&peerdb);
}

// Generate some Unreachable addresses, check that they are removed by 'heartbeat' once the
// corresponding conditions are met.
#[tracing::instrument(skip(seed))]
#[rstest]
#[trace]
#[case(Seed::from_entropy())]
fn remove_unreachable(#[case] seed: Seed) {
    let mut rng = make_seedable_rng(seed);

    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let chain_config = create_unit_test_config();
    let p2p_config = Arc::new(test_p2p_config());
    let peerdb_config = PeerDbConfig {
        addr_tables_bucket_size: 10.into(),
        new_addr_table_bucket_count: 10.into(),
        tried_addr_table_bucket_count: 10.into(),
        addr_tables_initial_random_key: Some(RandomKey::new_random_with_rng(&mut rng)),
    };

    let mut peerdb = PeerDb::new_with_config(
        &chain_config,
        Arc::clone(&p2p_config),
        &peerdb_config,
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    let addr_count = 10;

    let new_addrs = make_non_colliding_addresses(new_addr_table(&peerdb), addr_count, &mut rng);
    let tried_addrs = make_non_colliding_addresses(tried_addr_table(&peerdb), addr_count, &mut rng);
    let tried_addrs_as_set = tried_addrs.iter().copied().collect::<BTreeSet<_>>();

    for addr in &new_addrs {
        peerdb.peer_discovered(*addr);
    }

    for addr in &tried_addrs {
        peerdb.outbound_peer_connected(*addr);
    }

    assert_eq!(new_addr_table(&peerdb).addr_count(), addr_count);
    assert_eq!(tried_addr_table(&peerdb).addr_count(), addr_count);
    assert_addr_consistency(&peerdb);

    let (new_addrs_unreachable, new_addrs_reachable) = split_in_two_sets(&new_addrs, &mut rng);
    let (tried_addrs_unreachable, tried_addrs_reachable) =
        split_in_two_sets(&tried_addrs, &mut rng);

    for addr in &new_addrs_unreachable {
        peerdb.report_outbound_failure(*addr);
    }

    for addr in &tried_addrs_unreachable {
        peerdb.outbound_peer_disconnected(*addr);
        peerdb.report_outbound_failure(*addr);
    }

    assert_addr_consistency(&peerdb);

    time_getter.advance_time(PURGE_UNREACHABLE_TIME);
    peerdb.heartbeat();

    // The failed "new" addresses have been removed, but the "tried" ones are still there, because
    // they were reachable once.
    let new_addrs_remaining = new_addr_table(&peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    let tried_addrs_remaining =
        tried_addr_table(&peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    assert_eq_sets(new_addrs_remaining.iter(), new_addrs_reachable.iter());
    assert_eq_sets(tried_addrs_remaining.iter(), tried_addrs_as_set.iter());
    assert_addr_consistency(&peerdb);

    // Call report_outbound_failure until the fail count reaches the limit.
    for addr in &tried_addrs_unreachable {
        for _ in 0..PURGE_REACHABLE_FAIL_COUNT - 1 {
            peerdb.report_outbound_failure(*addr);
        }
    }

    time_getter.advance_time(PURGE_UNREACHABLE_TIME);
    peerdb.heartbeat();

    // Now the failed "tried" addresses are also removed.
    let new_addrs_remaining = new_addr_table(&peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    let tried_addrs_remaining =
        tried_addr_table(&peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    assert_eq_sets(new_addrs_remaining.iter(), new_addrs_reachable.iter());
    assert_eq_sets(tried_addrs_remaining.iter(), tried_addrs_reachable.iter());
    assert_addr_consistency(&peerdb);
}

// Check that "new" addresses are correctly evicted from the table when the address count limit
// is exceeded.
#[tracing::instrument(skip(seed))]
#[rstest]
#[trace]
#[case(Seed::from_entropy())]
fn new_addr_count_limit(#[case] seed: Seed, #[values(true, false)] use_reserved_nodes: bool) {
    let mut rng = make_seedable_rng(seed);

    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let chain_config = create_unit_test_config();
    let p2p_config = Arc::new(test_p2p_config());
    let bucket_size = 10;
    let bucket_count = 10;
    let max_addrs_in_one_table = bucket_count * bucket_size;
    let peerdb_config = PeerDbConfig {
        addr_tables_bucket_size: bucket_size.into(),
        new_addr_table_bucket_count: bucket_count.into(),
        tried_addr_table_bucket_count: bucket_count.into(),
        addr_tables_initial_random_key: Some(RandomKey::new_random_with_rng(&mut rng)),
    };

    let mut peerdb = PeerDb::new_with_config(
        &chain_config,
        Arc::clone(&p2p_config),
        &peerdb_config,
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    assert_eq!(new_addr_table(&peerdb).addr_count(), 0);
    assert_eq!(tried_addr_table(&peerdb).addr_count(), 0);

    for i in 0..max_addrs_in_one_table * 10 {
        let addr = make_random_address(&mut rng);

        if use_reserved_nodes && i % 3 == 0 {
            peerdb.add_reserved_node(addr);
        }

        peerdb.peer_discovered(addr);

        if use_reserved_nodes && i % 3 == 1 {
            peerdb.add_reserved_node(addr);
        }

        let new_addr_count = new_addr_table(&peerdb).addr_count();

        if !use_reserved_nodes || i >= 3 {
            assert!(new_addr_count > 0);
        }

        assert!(new_addr_count <= max_addrs_in_one_table);
        assert_eq!(tried_addr_table(&peerdb).addr_count(), 0);
        assert_addr_consistency(&peerdb);
    }
}

// Check that "tried" addresses are correctly evicted from the table when the address count limit
// is exceeded.
#[tracing::instrument(skip(seed))]
#[rstest]
#[trace]
#[case(Seed::from_entropy())]
fn tried_addr_count_limit(#[case] seed: Seed, #[values(true, false)] use_reserved_nodes: bool) {
    let mut rng = make_seedable_rng(seed);

    let db_store = peerdb_inmemory_store();
    let time_getter = P2pBasicTestTimeGetter::new();
    let chain_config = create_unit_test_config();
    let p2p_config = Arc::new(test_p2p_config());
    let bucket_size = 10;
    let bucket_count = 10;
    let max_addrs_in_one_table = bucket_count * bucket_size;
    let peerdb_config = PeerDbConfig {
        addr_tables_bucket_size: bucket_size.into(),
        new_addr_table_bucket_count: bucket_count.into(),
        tried_addr_table_bucket_count: bucket_count.into(),
        addr_tables_initial_random_key: Some(RandomKey::new_random_with_rng(&mut rng)),
    };

    let mut peerdb = PeerDb::new_with_config(
        &chain_config,
        Arc::clone(&p2p_config),
        &peerdb_config,
        time_getter.get_time_getter(),
        db_store,
    )
    .unwrap();

    assert_eq!(new_addr_table(&peerdb).addr_count(), 0);
    assert_eq!(tried_addr_table(&peerdb).addr_count(), 0);

    for i in 0..max_addrs_in_one_table * 10 {
        let addr = make_random_address(&mut rng);

        if use_reserved_nodes && i % 3 == 0 {
            peerdb.add_reserved_node(addr);
        }

        peerdb.outbound_peer_connected(addr);

        if use_reserved_nodes && i % 3 == 1 {
            peerdb.add_reserved_node(addr);
        }

        let tried_addr_count = tried_addr_table(&peerdb).addr_count();
        assert!(tried_addr_count > 0);
        assert!(tried_addr_count <= max_addrs_in_one_table);
        assert!(new_addr_table(&peerdb).addr_count() <= max_addrs_in_one_table);
        assert_addr_consistency(&peerdb);
    }
}

fn assert_eq_sets<T, I1, I2>(iter1: I1, iter2: I2)
where
    I1: Iterator<Item = T>,
    I2: Iterator<Item = T>,
    T: Eq + Debug,
{
    assert_eq!(iter1.zip_eq(iter2).find(|(val1, val2)| val1 != val2), None);
}

fn assert_eq_sets_if_not_in<T, I1, I2>(iter1: I1, iter2: I2, items_to_ignore: &BTreeSet<T>)
where
    I1: Iterator<Item = T>,
    I2: Iterator<Item = T>,
    T: Eq + Ord + Debug,
{
    assert_eq_sets(
        iter1.filter(|a| !items_to_ignore.contains(a)),
        iter2.filter(|a| !items_to_ignore.contains(a)),
    );
}

/// Split the passed items into two sets of random (but usually roughly equal) sizes.
/// The first set is guaranteed to be non-empty (unless `items` is itself empty).
fn split_in_two_sets<T>(items: &[T], rng: &mut impl Rng) -> (BTreeSet<T>, BTreeSet<T>)
where
    T: Eq + Ord + Clone,
{
    let mut first = BTreeSet::new();
    let mut second = BTreeSet::new();

    for (idx, item) in items.iter().enumerate() {
        let is_last = idx == items.len() - 1;
        if rng.gen::<u32>() % 2 == 0 || (is_last && first.is_empty()) {
            first.insert(item.clone());
        } else {
            second.insert(item.clone());
        }
    }

    (first, second)
}

fn assert_addr_consistency<S: PeerDbStorage>(peerdb: &PeerDb<S>) {
    // Check that addresses in the new table are distinct.
    let new_addr_count = new_addr_table(peerdb).addr_count();
    let new_addrs = new_addr_table(peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(new_addrs.len(), new_addr_count);
    // Check that addresses in the tried table are distinct.
    let tried_addr_count = tried_addr_table(peerdb).addr_count();
    let tried_addrs = tried_addr_table(peerdb).addr_iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(tried_addrs.len(), tried_addr_count);
    // Check that the tables are disjoint.
    assert!(new_addrs.is_disjoint(&tried_addrs));

    let addrs_in_both_tables = new_addrs.union(&tried_addrs).copied().collect::<BTreeSet<_>>();
    let db_addrs = {
        let tx = peerdb.storage.transaction_ro().unwrap();
        tx.get_known_addresses().unwrap().iter().copied().collect::<BTreeMap<_, _>>()
    };

    // Addresses in the db and in peerdb.addresses are the same, if not taking "reserved"
    // ones into account.
    assert_eq_sets_if_not_in(
        db_addrs.keys().copied(),
        peerdb.addresses.keys().copied(),
        &peerdb.reserved_nodes,
    );

    // Addresses in the db and in the tables are the same, if not taking "reserved"
    // ones into account.
    assert_eq_sets_if_not_in(
        db_addrs.keys().copied(),
        addrs_in_both_tables.iter().copied(),
        &peerdb.reserved_nodes,
    );

    // Check that all "reserved" addresses are also in peerdb.addresses.
    for addr in &peerdb.reserved_nodes {
        assert!(peerdb.addresses.contains_key(addr));
    }

    // Check that addresses in a table are represented in the db with the correct "state".
    for addr in &new_addrs {
        assert_eq!(*db_addrs.get(addr).unwrap(), KnownAddressState::New);
    }
    for addr in &tried_addrs {
        assert_eq!(*db_addrs.get(addr).unwrap(), KnownAddressState::Tried);
    }
}

fn new_addr_table<S>(peerdb: &PeerDb<S>) -> &Table {
    peerdb.address_tables.new_addr_table()
}

fn tried_addr_table<S>(peerdb: &PeerDb<S>) -> &Table {
    peerdb.address_tables.tried_addr_table()
}
