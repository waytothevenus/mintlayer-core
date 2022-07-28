// Copyright (c) 2022 RBB S.r.l
// opensource@mintlayer.org
// SPDX-License-Identifier: MIT
// Licensed under the MIT License;
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://spdx.org/licenses/MIT
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// Author(s): S. Afach, A. Sinitsyn, S. Tkach

use std::sync::Mutex;

use crate::detail::{tests::test_framework::TestFramework, *};
use chainstate_storage::Store;
use common::{
    chain::{
        block::{timestamp::BlockTimestamp, ConsensusData},
        config::{create_regtest, create_unit_test_config},
        signature::inputsig::InputWitness,
        Block, Destination, GenBlock, GenBlockId, Genesis, OutPointSourceId, OutputPurpose,
        Transaction, TxInput, TxOutput,
    },
    primitives::{time, Amount, BlockHeight, Id},
    Uint256,
};
use crypto::random::{Rng, SliceRandom};
use rstest::rstest;
use serialization::Encode;
use test_utils::random::{make_seedable_rng, Seed};

mod double_spend_tests;
mod events_tests;
mod output_timelock;
mod processing_tests;
mod reorgs_tests;
mod signature_tests;
mod syncing_tests;
mod test_framework;

type EventList = Arc<Mutex<Vec<(Id<Block>, BlockHeight)>>>;

const ERR_BEST_BLOCK_NOT_FOUND: &str = "Best block not found";
const ERR_STORAGE_FAIL: &str = "Storage failure";
const ERR_CREATE_BLOCK_FAIL: &str = "Creating block caused fail";
const ERR_CREATE_TX_FAIL: &str = "Creating tx caused fail";

fn empty_witness(rng: &mut impl Rng) -> InputWitness {
    let mut msg: Vec<u8> = (1..100).collect();
    msg.shuffle(rng);
    InputWitness::NoSignature(Some(msg))
}

fn anyonecanspend_address() -> Destination {
    Destination::AnyoneCanSpend
}

fn create_utxo_data(
    outsrc: OutPointSourceId,
    index: usize,
    output: &TxOutput,
    rng: &mut impl Rng,
) -> Option<(TxInput, TxOutput)> {
    let spent_value = Amount::from_atoms(rng.gen_range(0..output.value().into_atoms()));
    let new_value = (output.value() - spent_value).unwrap();
    utils::ensure!(new_value >= Amount::from_atoms(1));
    Some((
        TxInput::new(outsrc, index as u32, empty_witness(rng)),
        TxOutput::new(new_value, OutputPurpose::Transfer(anyonecanspend_address())),
    ))
}

fn setup_chainstate() -> Chainstate {
    chainstate_with_config(create_unit_test_config(), ChainstateConfig::new())
}

fn chainstate_with_config(
    chain_config: ChainConfig,
    chainstate_config: ChainstateConfig,
) -> Chainstate {
    Chainstate::new(
        Arc::new(chain_config),
        chainstate_config,
        Store::new_empty().unwrap(),
        None,
        Default::default(),
    )
    .unwrap()
}

// TODO: Replace by a proper UTXO set abstraction
// (https://github.com/mintlayer/mintlayer-core/issues/312).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestBlockInfo {
    pub(crate) txns: Vec<(OutPointSourceId, Vec<TxOutput>)>,
    pub(crate) id: Id<GenBlock>,
}

impl TestBlockInfo {
    fn from_block(blk: &Block) -> Self {
        let txns = blk
            .transactions()
            .iter()
            .map(|tx| {
                (
                    OutPointSourceId::Transaction(tx.get_id()),
                    tx.outputs().clone(),
                )
            })
            .collect();
        let id = blk.get_id().into();
        Self { txns, id }
    }

    fn from_genesis(genesis: &Genesis) -> Self {
        let id: Id<GenBlock> = genesis.get_id().into();
        let outsrc = OutPointSourceId::BlockReward(id);
        let txns = vec![(outsrc, genesis.utxos().to_vec())];
        Self { txns, id }
    }

    fn from_id(cs: &Chainstate, id: Id<GenBlock>) -> Self {
        use chainstate_storage::BlockchainStorageRead;
        match id.classify(&cs.chain_config) {
            GenBlockId::Genesis(_) => Self::from_genesis(cs.chain_config.genesis_block()),
            GenBlockId::Block(id) => {
                let block = cs.chainstate_storage.get_block(id).unwrap().unwrap();
                Self::from_block(&block)
            }
        }
    }
}

// TODO: FIXME: Remove?..
fn produce_test_block(prev_block: TestBlockInfo, rng: &mut impl Rng) -> Block {
    produce_test_block_with_consensus_data(prev_block, ConsensusData::None, rng)
}

fn produce_test_block_with_consensus_data(
    prev_block: TestBlockInfo,
    consensus_data: ConsensusData,
    rng: &mut impl Rng,
) -> Block {
    // The value of each output is decreased by a random amount to produce a new input and output.
    let (inputs, outputs): (Vec<TxInput>, Vec<TxOutput>) = prev_block
        .txns
        .into_iter()
        .flat_map(|(s, o)| create_new_outputs(s, &o, rng))
        .unzip();

    Block::new(
        vec![Transaction::new(0, inputs, outputs, 0).expect(ERR_CREATE_TX_FAIL)],
        prev_block.id,
        BlockTimestamp::from_duration_since_epoch(time::get()),
        consensus_data,
    )
    .expect(ERR_CREATE_BLOCK_FAIL)
}

fn create_new_outputs(
    srcid: OutPointSourceId,
    outs: &[TxOutput],
    rng: &mut impl Rng,
) -> Vec<(TxInput, TxOutput)> {
    outs.iter()
        .enumerate()
        .filter_map(move |(index, output)| create_utxo_data(srcid.clone(), index, output, rng))
        .collect()
}

// Generate 5 regtest blocks and print their hex encoding, which is useful for functional tests.
// TODO: remove when block production is ready
#[ignore]
#[rstest]
#[trace]
#[case(Seed::from_entropy())]
fn generate_blocks_for_functional_tests(#[case] seed: Seed) {
    let mut rng = make_seedable_rng(seed);
    let chain_config = create_regtest();
    let mut prev_block = TestBlockInfo::from_genesis(chain_config.genesis_block());
    let chainstate_config = ChainstateConfig::new();
    let chainstate = chainstate_with_config(chain_config, chainstate_config);
    let mut btf = TestFramework::with_chainstate(chainstate);
    let difficulty =
        Uint256([0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 0xFFFFFFFFFFFFFFFF, 0x7FFFFFFFFFFFFFFF]);

    for _ in 1..6 {
        let mut mined_block = btf.random_block(prev_block, None, &mut rng);
        let bits = difficulty.into();
        assert!(
            crate::detail::pow::work::mine(&mut mined_block, u128::MAX, bits, vec![])
                .expect("Unexpected conversion error")
        );
        println!("{}", hex::encode(mined_block.encode()));
        prev_block = TestBlockInfo::from_block(&mined_block);
        btf.add_special_block(mined_block).unwrap();
    }
}
