use chainstate::ConnectTransactionError;
use chainstate::{
    BlockError, ChainstateError, CheckBlockError, CheckBlockTransactionsError, IOPolicyError,
};
use chainstate_test_framework::{TestFramework, TransactionBuilder};
use common::chain::OutPointSourceId;
use common::chain::{
    signature::inputsig::InputWitness, tokens::TokenIssuanceVersion, ChainstateUpgrade,
    Destination, TxInput, TxOutput,
};
use common::primitives::Amount;
use common::primitives::{BlockHeight, Idable};
use crypto::random::Rng;
use rstest::rstest;
use test_utils::random::{make_seedable_rng, Seed};

#[rstest]
#[trace]
#[case(Seed::from_entropy(), 1.into(), true)]
#[trace]
#[case(Seed::from_entropy(), 10.into(), false)]
fn data_deposit_fork_height(
    #[case] seed: Seed,
    #[case] activation_height: BlockHeight,
    #[case] expect_success: bool,
) {
    utils::concurrency::model(move || {
        let mut rng = make_seedable_rng(seed);
        let mut tf = TestFramework::builder(&mut rng)
            .with_chain_config(
                common::chain::config::Builder::test_chain()
                    .chainstate_upgrades(
                        common::chain::NetUpgrades::initialize(vec![
                            (
                                BlockHeight::zero(),
                                ChainstateUpgrade::new(TokenIssuanceVersion::V0),
                            ),
                            (
                                activation_height,
                                ChainstateUpgrade::new(TokenIssuanceVersion::V1),
                            ),
                        ])
                        .unwrap(),
                    )
                    .genesis_unittest(Destination::AnyoneCanSpend)
                    .build(),
            )
            .build();
        let outpoint_source_id = OutPointSourceId::BlockReward(tf.genesis().get_id().into());
        let mut rng = make_seedable_rng(seed);

        let deposited_data_len = tf.chain_config().data_deposit_max_size();
        let deposited_data_len = rng.gen_range(0..deposited_data_len);
        let deposited_data = (0..deposited_data_len).map(|_| rng.gen::<u8>()).collect::<Vec<_>>();

        let tx = TransactionBuilder::new()
            .add_input(
                TxInput::from_utxo(outpoint_source_id, 0),
                InputWitness::NoSignature(None),
            )
            .add_output(TxOutput::DataDeposit(deposited_data))
            .build();

        let block = tf.make_block_builder().add_transaction(tx.clone()).build();

        if expect_success {
            let _new_connected_block_index = tf
                .process_block(block.clone(), chainstate::BlockSource::Local)
                .unwrap()
                .unwrap();
        } else {
            let result = tf.process_block(block.clone(), chainstate::BlockSource::Local);

            let expected_err = Err(ChainstateError::ProcessBlockError(
                BlockError::CheckBlockFailed(CheckBlockError::CheckTransactionFailed(
                    CheckBlockTransactionsError::DataDepositNotActivated(
                        1.into(),
                        tx.transaction().get_id(),
                        block.get_id(),
                    ),
                )),
            ));

            assert_eq!(result, expected_err);
        }
    })
}

#[rstest]
#[trace]
#[case(Seed::from_entropy(), true)]
#[trace]
#[case(Seed::from_entropy(), false)]
fn data_deposited_too_large(#[case] seed: Seed, #[case] expect_success: bool) {
    utils::concurrency::model(move || {
        let mut rng = make_seedable_rng(seed);
        let mut tf = TestFramework::builder(&mut rng)
            .with_chain_config(
                common::chain::config::Builder::test_chain()
                    .chainstate_upgrades(
                        common::chain::NetUpgrades::initialize(vec![(
                            BlockHeight::zero(),
                            ChainstateUpgrade::new(TokenIssuanceVersion::V1),
                        )])
                        .unwrap(),
                    )
                    .genesis_unittest(Destination::AnyoneCanSpend)
                    .build(),
            )
            .build();
        let outpoint_source_id = OutPointSourceId::BlockReward(tf.genesis().get_id().into());
        let mut rng = make_seedable_rng(seed);

        let deposited_data_len = if expect_success {
            tf.chain_config().data_deposit_max_size()
        } else {
            tf.chain_config().data_deposit_max_size() + 1
        };
        let deposited_data = (0..deposited_data_len).map(|_| rng.gen::<u8>()).collect::<Vec<_>>();

        let tx = TransactionBuilder::new()
            .add_input(
                TxInput::from_utxo(outpoint_source_id, 0),
                InputWitness::NoSignature(None),
            )
            .add_output(TxOutput::DataDeposit(deposited_data))
            .build();

        let block = tf.make_block_builder().add_transaction(tx.clone()).build();

        if expect_success {
            let _new_connected_block_index = tf
                .process_block(block.clone(), chainstate::BlockSource::Local)
                .unwrap()
                .unwrap();
        } else {
            let result = tf.process_block(block.clone(), chainstate::BlockSource::Local);

            let expected_err = Err(ChainstateError::ProcessBlockError(
                BlockError::CheckBlockFailed(CheckBlockError::CheckTransactionFailed(
                    CheckBlockTransactionsError::DataDepositMaxSizeExceeded(
                        deposited_data_len,
                        tf.chain_config().data_deposit_max_size(),
                        tx.transaction().get_id(),
                        block.get_id(),
                    ),
                )),
            ));

            assert_eq!(result, expected_err);
        }
    })
}

#[rstest]
#[trace]
#[case(Seed::from_entropy(), true)]
#[trace]
#[case(Seed::from_entropy(), false)]
fn data_deposit_insufficient_fee(#[case] seed: Seed, #[case] expect_success: bool) {
    utils::concurrency::model(move || {
        let mut rng = make_seedable_rng(seed);
        let mut tf = TestFramework::builder(&mut rng)
            .with_chain_config(
                common::chain::config::Builder::test_chain()
                    .chainstate_upgrades(
                        common::chain::NetUpgrades::initialize(vec![(
                            BlockHeight::zero(),
                            ChainstateUpgrade::new(TokenIssuanceVersion::V1),
                        )])
                        .unwrap(),
                    )
                    .genesis_unittest(Destination::AnyoneCanSpend)
                    .build(),
            )
            .build();
        let outpoint_source_id = OutPointSourceId::BlockReward(tf.genesis().get_id().into());
        let mut rng = make_seedable_rng(seed);

        let deposited_data_len = tf.chain_config().data_deposit_max_size();
        let deposited_data_len = rng.gen_range(0..deposited_data_len);
        let deposited_data = (0..deposited_data_len).map(|_| rng.gen::<u8>()).collect::<Vec<_>>();

        let data_fee = if expect_success {
            tf.chain_config().data_deposit_min_fee()
        } else {
            (tf.chain_config().data_deposit_min_fee() - Amount::from_atoms(1)).unwrap()
        };

        let tx_with_fee_as_output = TransactionBuilder::new()
            .add_input(
                TxInput::from_utxo(outpoint_source_id, 0),
                InputWitness::NoSignature(None),
            )
            .add_output(TxOutput::Transfer(
                common::chain::output_value::OutputValue::Coin(data_fee),
                Destination::AnyoneCanSpend,
            ))
            .build();

        // First block creates an output with the specified amount
        let _block_index = tf
            .make_block_builder()
            .add_transaction(tx_with_fee_as_output.clone())
            .build_and_process()
            .unwrap()
            .unwrap();

        let tx = TransactionBuilder::new()
            .add_input(
                TxInput::from_utxo(tx_with_fee_as_output.transaction().get_id().into(), 0),
                InputWitness::NoSignature(None),
            )
            .add_output(TxOutput::DataDeposit(deposited_data))
            .build();

        let block = tf.make_block_builder().add_transaction(tx.clone()).build();

        if expect_success {
            let _new_connected_block_index = tf
                .process_block(block.clone(), chainstate::BlockSource::Local)
                .unwrap()
                .unwrap();
        } else {
            let result = tf.process_block(block.clone(), chainstate::BlockSource::Local);

            let expected_err = Err(ChainstateError::ProcessBlockError(
                BlockError::StateUpdateFailed(ConnectTransactionError::IOPolicyError(
                    IOPolicyError::AttemptToPrintMoneyOrViolateTimelockConstraints,
                    OutPointSourceId::Transaction(tx.transaction().get_id()),
                )),
            ));

            assert_eq!(result, expected_err);
        }
    })
}
