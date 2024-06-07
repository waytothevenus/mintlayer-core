// Copyright (c) 2023 RBB S.r.l
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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::account::transaction_list::TransactionList;
use crate::account::TxInfo;
use crate::account::{
    currency_grouper::Currency, CurrentFeeRate, DelegationData, PartiallySignedTransaction,
    PoolData, TransactionToSign, UnconfirmedTokenInfo, UtxoSelectorError,
};
use crate::key_chain::{
    make_account_path, make_path_to_vrf_key, KeyChainError, MasterKeyChain, LOOKAHEAD_SIZE,
    VRF_INDEX,
};
use crate::send_request::{
    make_issue_token_outputs, IssueNftArguments, SelectedInputs, StakePoolDataArguments,
};
use crate::signer::software_signer::SoftwareSigner;
use crate::signer::{Signer, SignerError};
use crate::wallet_events::{WalletEvents, WalletEventsNoOp};
use crate::{Account, SendRequest};
pub use bip39::{Language, Mnemonic};
use common::address::pubkeyhash::PublicKeyHash;
use common::address::{Address, AddressError, RpcAddress};
use common::chain::block::timestamp::BlockTimestamp;
use common::chain::classic_multisig::ClassicMultisigChallenge;
use common::chain::signature::inputsig::arbitrary_message::{
    ArbitraryMessageSignature, SignArbitraryMessageError,
};
use common::chain::signature::DestinationSigError;
use common::chain::tokens::{
    make_token_id, IsTokenUnfreezable, Metadata, RPCFungibleTokenInfo, TokenId, TokenIssuance,
};
use common::chain::{
    AccountNonce, Block, ChainConfig, DelegationId, Destination, GenBlock, PoolId,
    SignedTransaction, Transaction, TransactionCreationError, TxInput, TxOutput, UtxoOutPoint,
};
use common::primitives::id::{hash_encoded, WithId};
use common::primitives::{Amount, BlockHeight, Id, H256};
use common::size_estimation::SizeEstimationError;
use consensus::PoSGenerateBlockInputData;
use crypto::key::hdkd::child_number::ChildNumber;
use crypto::key::hdkd::derivable::Derivable;
use crypto::key::hdkd::u31::U31;
use crypto::key::{PrivateKey, PublicKey};
use crypto::vrf::VRFPublicKey;
use mempool::FeeRate;
use pos_accounting::make_delegation_id;
use tx_verifier::error::TokenIssuanceError;
use tx_verifier::{check_transaction, CheckTransactionError};
use utils::ensure;
pub use wallet_storage::Error;
use wallet_storage::{
    DefaultBackend, Store, StoreTxRw, StoreTxRwUnlocked, TransactionRoLocked, TransactionRwLocked,
    TransactionRwUnlocked, Transactional, WalletStorageReadLocked, WalletStorageReadUnlocked,
    WalletStorageWriteLocked, WalletStorageWriteUnlocked,
};
use wallet_types::account_info::{StandaloneAddressDetails, StandaloneAddresses};
use wallet_types::chain_info::ChainInfo;
use wallet_types::seed_phrase::{SerializableSeedPhrase, StoreSeedPhrase};
use wallet_types::signature_status::SignatureStatus;
use wallet_types::utxo_types::{UtxoStates, UtxoTypes};
use wallet_types::wallet_tx::{TxData, TxState};
use wallet_types::wallet_type::WalletType;
use wallet_types::with_locked::WithLocked;
use wallet_types::{AccountId, AccountKeyPurposeId, BlockInfo, KeyPurpose, KeychainUsageState};

pub const WALLET_VERSION_UNINITIALIZED: u32 = 0;
pub const WALLET_VERSION_V1: u32 = 1;
pub const WALLET_VERSION_V2: u32 = 2;
pub const WALLET_VERSION_V3: u32 = 3;
pub const WALLET_VERSION_V4: u32 = 4;
pub const WALLET_VERSION_V5: u32 = 5;
pub const WALLET_VERSION_V6: u32 = 6;
pub const WALLET_VERSION_V7: u32 = 7;
pub const CURRENT_WALLET_VERSION: u32 = WALLET_VERSION_V7;

/// Wallet errors
#[derive(thiserror::Error, Debug, Eq, PartialEq)]
pub enum WalletError {
    #[error("Wallet is not initialized")]
    WalletNotInitialized,
    #[error("A {0} wallet is trying to open a {1} wallet file")]
    DifferentWalletType(WalletType, WalletType),
    #[error("The wallet belongs to a different chain than the one specified")]
    DifferentChainType,
    #[error("Unsupported wallet version: {0}, max supported version of this software is {CURRENT_WALLET_VERSION}")]
    UnsupportedWalletVersion(u32),
    #[error("Wallet database error: {0}")]
    DatabaseError(#[from] wallet_storage::Error),
    #[error("Transaction already present: {0}")]
    DuplicateTransaction(Id<Transaction>),
    #[error("No transaction found: {0}")]
    NoTransactionFound(Id<Transaction>),
    #[error("Key chain error: {0}")]
    KeyChainError(#[from] KeyChainError),
    #[error("No account found")] // TODO implement display for AccountId
    NoAccountFound(AccountId),
    #[error("No account found with index {0}")]
    NoAccountFoundWithIndex(U31),
    #[error("Account with index {0} already exists")]
    AccountAlreadyExists(U31),
    #[error("Cannot create a new account when last account is still empty")]
    EmptyLastAccount,
    #[error("Cannot create a new account with an empty string name")]
    EmptyAccountName,
    #[error("The maximum number of accounts has been exceeded: {0}")]
    AbsoluteMaxNumAccountsExceeded(U31),
    #[error("Not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("Unsupported transaction output type")] // TODO implement display for TxOutput
    UnsupportedTransactionOutput(Box<TxOutput>),
    #[error("Size estimation error: {0}")]
    SizeEstimationError(#[from] SizeEstimationError),
    #[error("Output amounts overflow")]
    OutputAmountOverflow,
    #[error("Delegation with id: {0} with duplicate AccountNonce: {1}")]
    InconsistentDelegationDuplicateNonce(DelegationId, AccountNonce),
    #[error("Inconsistent produce block from stake for pool id: {0}, missing CreateStakePool")]
    InconsistentProduceBlockFromStake(PoolId),
    #[error("Delegation nonce overflow for id: {0}")]
    DelegationNonceOverflow(DelegationId),
    #[error("Token issuance nonce overflow for id: {0}")]
    TokenIssuanceNonceOverflow(TokenId),
    #[error("Token with id: {0} with duplicate AccountNonce: {1}")]
    InconsistentTokenIssuanceDuplicateNonce(TokenId, AccountNonce),
    #[error("Empty inputs in token issuance transaction")]
    MissingTokenId,
    #[error("Unknown token with Id {0}")]
    UnknownTokenId(TokenId),
    #[error("Transaction creation error: {0}")]
    TransactionCreation(#[from] TransactionCreationError),
    #[error("Transaction signing error: {0}")]
    TransactionSig(#[from] DestinationSigError),
    #[error("Delegation not found with id {0}")]
    DelegationNotFound(DelegationId),
    #[error("Not enough UTXOs amount: {0:?}, required: {1:?}")]
    NotEnoughUtxo(Amount, Amount),
    #[error("Token issuance error: {0}")]
    TokenIssuance(#[from] TokenIssuanceError),
    #[error("{0}")]
    InvalidTransaction(#[from] CheckTransactionError),
    #[error("No UTXOs")]
    NoUtxos,
    #[error("Coin selection error: {0}")]
    CoinSelectionError(#[from] UtxoSelectorError),
    #[error("Cannot abandon a transaction in {0} state")]
    CannotAbandonTransaction(TxState),
    #[error("Transaction with Id {0} not found")]
    CannotFindTransactionWithId(Id<Transaction>),
    #[error("Address error: {0}")]
    AddressError(#[from] AddressError),
    #[error("Unknown pool id {0}")]
    UnknownPoolId(PoolId),
    #[error("Cannot find UTXO")]
    CannotFindUtxo(UtxoOutPoint),
    #[error("Selected UTXO is already consumed")]
    ConsumedUtxo(UtxoOutPoint),
    #[error("Selected UTXO is still locked")]
    LockedUtxo(UtxoOutPoint),
    #[error("Selected UTXO is a token v0 and cannot be used")]
    TokenV0Utxo(UtxoOutPoint),
    #[error("Cannot change a Locked Token supply")]
    CannotChangeLockedTokenSupply,
    #[error("Cannot lock Token supply in state: {0}")]
    CannotLockTokenSupply(&'static str),
    #[error("Cannot revert lock Token supply in state: {0}")]
    InconsistentUnlockTokenSupply(&'static str),
    #[error(
        "Cannot mint Token over the fixed supply {0:?}, current supply {1:?} trying to mint {2:?}"
    )]
    CannotMintFixedTokenSupply(Amount, Amount, Amount),
    #[error("Trying to unmint {0:?} but the current supply is {1:?}")]
    CannotUnmintTokenSupply(Amount, Amount),
    #[error("Cannot freeze a not freezable token")]
    CannotFreezeNotFreezableToken,
    #[error("Cannot freeze an already frozen token")]
    CannotFreezeAlreadyFrozenToken,
    #[error("Cannot unfreeze this token")]
    CannotUnfreezeToken,
    #[error("Cannot unfreeze a not frozen token")]
    CannotUnfreezeANotFrozenToken,
    #[error("Cannot use a frozen token")]
    CannotUseFrozenToken,
    #[error("Cannot change a not owned token")]
    CannotChangeNotOwnedToken(TokenId),
    #[error("Cannot change a non-fungible token")]
    CannotChangeNonFungibleToken(TokenId),
    #[error("The size of the data to be deposited: {0} is too big, max size is: {1}")]
    DataDepositToBig(usize, usize),
    #[error("Cannot deposit empty data")]
    EmptyDataDeposit,
    #[error("Cannot reduce lookahead size to {0} as it is below the last known used key {1}")]
    ReducedLookaheadSize(u32, u32),
    #[error("Wallet file {0} error: {1}")]
    WalletFileError(PathBuf, String),
    #[error("Failed to completely sign the decommission transaction. \
            This wallet does not seem to have the decommission key. \
            Consider using a decommission-request, and passing it to the wallet that has the decommission key")]
    PartiallySignedTransactionInDecommissionCommand,
    #[error("Failed to create decommission request as all the signatures are present. Use staking-decommission-pool command.")]
    FullySignedTransactionInDecommissionReq,
    #[error("Destination does not belong to this wallet")]
    DestinationNotFromThisWallet,
    #[error("Sign message error: {0}")]
    SignMessageError(#[from] SignArbitraryMessageError),
    #[error("Input cannot be spent {0:?}")]
    InputCannotBeSpent(TxOutput),
    #[error("Failed to convert partially signed tx to signed")]
    FailedToConvertPartiallySignedTx(PartiallySignedTransaction),
    #[error("The specified address is not found in this wallet")]
    AddressNotFound,
    #[error("The specified standalone address {0} is not found in this wallet")]
    StandaloneAddressNotFound(RpcAddress<Destination>),
    #[error("Signer error: {0}")]
    SignerError(#[from] SignerError),
}

/// Result type used for the wallet
pub type WalletResult<T> = Result<T, WalletError>;

/// Wallet tracks all pools that can be decommissioned or used for staking by the wallet.
/// Filter allows to query for a specific set of pools.
pub enum WalletPoolsFilter {
    All,
    Decommission,
    Stake,
}

pub struct Wallet<B: storage::Backend> {
    chain_config: Arc<ChainConfig>,
    db: Store<B>,
    key_chain: MasterKeyChain,
    accounts: BTreeMap<U31, Account>,
    latest_median_time: BlockTimestamp,
    next_unused_account: (U31, Account),
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct WalletSyncingState {
    pub account_best_blocks: BTreeMap<U31, (Id<GenBlock>, BlockHeight)>,
    pub unused_account_best_block: (Id<GenBlock>, BlockHeight),
}

pub fn open_or_create_wallet_file<P: AsRef<Path>>(path: P) -> WalletResult<Store<DefaultBackend>> {
    Ok(Store::new(DefaultBackend::new(path))?)
}

pub fn create_wallet_in_memory() -> WalletResult<Store<DefaultBackend>> {
    Ok(Store::new(DefaultBackend::new_in_memory())?)
}

impl<B: storage::Backend> Wallet<B> {
    pub fn create_new_wallet(
        chain_config: Arc<ChainConfig>,
        db: Store<B>,
        mnemonic: &str,
        passphrase: Option<&str>,
        save_seed_phrase: StoreSeedPhrase,
        best_block: (BlockHeight, Id<GenBlock>),
        wallet_type: WalletType,
    ) -> WalletResult<Self> {
        let mut wallet = Self::new_wallet(
            chain_config,
            db,
            mnemonic,
            passphrase,
            save_seed_phrase,
            wallet_type,
        )?;

        wallet.set_best_block(best_block.0, best_block.1)?;

        Ok(wallet)
    }

    pub fn recover_wallet(
        chain_config: Arc<ChainConfig>,
        db: Store<B>,
        mnemonic: &str,
        passphrase: Option<&str>,
        save_seed_phrase: StoreSeedPhrase,
        wallet_type: WalletType,
    ) -> WalletResult<Self> {
        Self::new_wallet(
            chain_config,
            db,
            mnemonic,
            passphrase,
            save_seed_phrase,
            wallet_type,
        )
    }

    fn new_wallet(
        chain_config: Arc<ChainConfig>,
        db: Store<B>,
        mnemonic: &str,
        passphrase: Option<&str>,
        save_seed_phrase: StoreSeedPhrase,
        wallet_type: WalletType,
    ) -> WalletResult<Self> {
        let mut db_tx = db.transaction_rw_unlocked(None)?;

        let key_chain = MasterKeyChain::new_from_mnemonic(
            chain_config.clone(),
            &mut db_tx,
            mnemonic,
            passphrase,
            save_seed_phrase,
        )?;

        db_tx.set_storage_version(CURRENT_WALLET_VERSION)?;
        db_tx.set_chain_info(&ChainInfo::new(chain_config.as_ref()))?;
        db_tx.set_lookahead_size(LOOKAHEAD_SIZE)?;
        db_tx.set_wallet_type(wallet_type)?;

        let default_account = Wallet::<B>::create_next_unused_account(
            U31::ZERO,
            chain_config.clone(),
            &key_chain,
            &mut db_tx,
            None,
        )?;

        let next_unused_account = Wallet::<B>::create_next_unused_account(
            U31::ONE,
            chain_config.clone(),
            &key_chain,
            &mut db_tx,
            None,
        )?;

        db_tx.commit()?;

        let latest_median_time = chain_config.genesis_block().timestamp();
        let wallet = Wallet {
            chain_config,
            db,
            key_chain,
            accounts: [default_account].into(),
            latest_median_time,
            next_unused_account,
        };

        Ok(wallet)
    }

    /// Migrate the wallet DB from version 1 to version 2
    /// * save the chain info in the DB based on the chain type specified by the user
    /// * reset transactions
    fn migration_v2(db: &Store<B>, chain_config: Arc<ChainConfig>) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw_unlocked(None)?;
        // set new chain info to the one provided by the user assuming it is the correct one
        db_tx.set_chain_info(&ChainInfo::new(chain_config.as_ref()))?;

        // reset wallet transaction as now we will need to rescan the blockchain to store the
        // correct order of the transactions to avoid bugs in loading them in the wrong order
        Self::reset_wallet_transactions(chain_config.clone(), &mut db_tx)?;

        // Create the next unused account
        Self::migrate_next_unused_account(chain_config, &mut db_tx)?;

        db_tx.set_storage_version(WALLET_VERSION_V2)?;
        db_tx.commit()?;
        logging::log::info!(
            "Successfully migrated wallet database to latest version {}",
            WALLET_VERSION_V2
        );

        Ok(())
    }

    /// Migrate the wallet DB from version 2 to version 3
    /// * reset transactions as now we store SignedTransaction instead of Transaction in WalletTx
    fn migration_v3(db: &Store<B>, chain_config: Arc<ChainConfig>) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw_unlocked(None)?;
        // reset wallet transaction as now we will need to rescan the blockchain to store the
        // correct order of the transactions to avoid bugs in loading them in the wrong order
        Self::reset_wallet_transactions(chain_config.clone(), &mut db_tx)?;

        db_tx.set_storage_version(WALLET_VERSION_V3)?;
        db_tx.commit()?;
        logging::log::info!(
            "Successfully migrated wallet database to latest version {}",
            WALLET_VERSION_V3
        );

        Ok(())
    }

    /// Migrate the wallet DB from version 3 to version 4
    /// * set lookahead_size in the DB
    fn migration_v4(db: &Store<B>) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw_unlocked(None)?;

        db_tx.set_lookahead_size(LOOKAHEAD_SIZE)?;
        db_tx.set_storage_version(WALLET_VERSION_V4)?;
        db_tx.commit()?;
        logging::log::info!(
            "Successfully migrated wallet database to latest version {}",
            WALLET_VERSION_V4
        );

        Ok(())
    }

    /// Migrate the wallet DB from version 4 to version 5
    /// * set vrf key_chain usage
    fn migration_v5(db: &Store<B>, chain_config: Arc<ChainConfig>) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw_unlocked(None)?;

        for (id, info) in db_tx.get_accounts_info()? {
            let root_vrf_key = MasterKeyChain::load_root_vrf_key(&db_tx)?;
            let account_path = make_account_path(&chain_config, info.account_index());
            let legacy_key_path = make_path_to_vrf_key(&chain_config, info.account_index());
            let legacy_vrf_key = root_vrf_key
                .clone()
                .derive_absolute_path(&legacy_key_path)
                .map_err(|err| WalletError::KeyChainError(KeyChainError::Derivation(err)))?
                .to_public_key();

            let account_vrf_pub_key = root_vrf_key
                .derive_absolute_path(&account_path)
                .map_err(|err| WalletError::KeyChainError(KeyChainError::Derivation(err)))?
                .derive_child(VRF_INDEX)
                .map_err(|err| WalletError::KeyChainError(KeyChainError::Derivation(err)))?
                .to_public_key();

            db_tx.set_account_vrf_public_keys(
                &id,
                &wallet_types::account_info::AccountVrfKeys {
                    account_vrf_key: account_vrf_pub_key.clone(),
                    legacy_vrf_key: legacy_vrf_key.clone(),
                },
            )?;
        }

        Self::reset_wallet_transactions_and_load(chain_config.clone(), &mut db_tx)?;

        db_tx.set_storage_version(WALLET_VERSION_V5)?;
        db_tx.commit()?;
        logging::log::info!(
            "Successfully migrated wallet database to latest version {}",
            WALLET_VERSION_V5
        );

        Ok(())
    }

    fn migration_v6(db: &Store<B>, _chain_config: Arc<ChainConfig>) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw(None)?;
        // nothing to do the seed phrase na passphrase are backwards compatible
        db_tx.set_storage_version(WALLET_VERSION_V6)?;
        db_tx.commit()?;

        logging::log::info!(
            "Successfully migrated wallet database to latest version {}",
            WALLET_VERSION_V6
        );
        Ok(())
    }

    fn migration_v7(
        db: &Store<B>,
        chain_config: Arc<ChainConfig>,
        wallet_type: WalletType,
    ) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw(None)?;
        let accs = db_tx.get_accounts_info()?;
        // if all accounts are still on genesis this is a cold wallet
        let cold_wallet =
            accs.values().all(|acc| acc.best_block_id() == chain_config.genesis_block_id());

        ensure!(
            wallet_type == WalletType::Hot || cold_wallet,
            WalletError::DifferentWalletType(wallet_type, WalletType::Hot)
        );

        db_tx.set_wallet_type(wallet_type)?;

        db_tx.set_storage_version(WALLET_VERSION_V7)?;
        db_tx.commit()?;

        logging::log::info!(
            "Successfully migrated wallet database to latest version {}",
            WALLET_VERSION_V7
        );
        Ok(())
    }

    /// Check the wallet DB version and perform any migrations needed
    fn check_and_migrate_db<F: Fn(u32) -> Result<(), WalletError>>(
        db: &Store<B>,
        chain_config: Arc<ChainConfig>,
        pre_migration: F,
        wallet_type: WalletType,
    ) -> WalletResult<()> {
        let version = db.transaction_ro()?.get_storage_version()?;

        match version {
            WALLET_VERSION_UNINITIALIZED => return Err(WalletError::WalletNotInitialized),
            WALLET_VERSION_V1 => {
                pre_migration(WALLET_VERSION_V1)?;
                Self::migration_v2(db, chain_config.clone())?;
            }
            WALLET_VERSION_V2 => {
                pre_migration(WALLET_VERSION_V2)?;
                Self::migration_v3(db, chain_config.clone())?;
            }
            WALLET_VERSION_V3 => {
                pre_migration(WALLET_VERSION_V3)?;
                Self::migration_v4(db)?;
            }
            WALLET_VERSION_V4 => {
                pre_migration(WALLET_VERSION_V4)?;
                Self::migration_v5(db, chain_config.clone())?;
            }
            WALLET_VERSION_V5 => {
                pre_migration(WALLET_VERSION_V5)?;
                Self::migration_v6(db, chain_config.clone())?;
            }
            WALLET_VERSION_V6 => {
                pre_migration(WALLET_VERSION_V6)?;
                Self::migration_v7(db, chain_config.clone(), wallet_type)?;
            }
            CURRENT_WALLET_VERSION => return Ok(()),
            unsupported_version => {
                return Err(WalletError::UnsupportedWalletVersion(unsupported_version))
            }
        }

        Self::check_and_migrate_db(db, chain_config, pre_migration, wallet_type)
    }

    fn validate_chain_info(
        chain_config: &ChainConfig,
        db_tx: &impl WalletStorageReadLocked,
        wallet_type: WalletType,
    ) -> WalletResult<()> {
        let chain_info = db_tx.get_chain_info()?;
        ensure!(
            chain_info.is_same(chain_config),
            WalletError::DifferentChainType
        );

        let this_wallet_type = db_tx.get_wallet_type()?;
        ensure!(
            this_wallet_type == wallet_type,
            WalletError::DifferentWalletType(wallet_type, this_wallet_type)
        );

        Ok(())
    }

    fn migrate_cold_to_hot_wallet(db: &Store<B>) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw(None)?;
        db_tx.set_wallet_type(WalletType::Hot)?;
        db_tx.commit()?;
        Ok(())
    }

    fn migrate_hot_to_cold_wallet(
        db: &Store<B>,
        chain_config: Arc<ChainConfig>,
    ) -> WalletResult<()> {
        let mut db_tx = db.transaction_rw(None)?;
        db_tx.set_wallet_type(WalletType::Cold)?;
        Self::reset_wallet_transactions_and_load(chain_config, &mut db_tx)?;
        db_tx.commit()?;
        Ok(())
    }

    fn force_migrate_wallet_type(
        wallet_type: WalletType,
        db: &Store<B>,
        chain_config: Arc<ChainConfig>,
    ) -> Result<(), WalletError> {
        let current_wallet_type = db.transaction_ro()?.get_wallet_type()?;
        match (current_wallet_type, wallet_type) {
            (WalletType::Cold, WalletType::Hot) => Self::migrate_cold_to_hot_wallet(db)?,
            (WalletType::Hot, WalletType::Cold) => {
                Self::migrate_hot_to_cold_wallet(db, chain_config)?
            }
            (WalletType::Cold, WalletType::Cold) => {}
            (WalletType::Hot, WalletType::Hot) => {}
        }
        Ok(())
    }

    /// Reset all scanned transactions and revert all accounts to the genesis block
    /// this will cause the wallet to rescan the blockchain
    pub fn reset_wallet_to_genesis(&mut self) -> WalletResult<()> {
        logging::log::info!(
            "Resetting the wallet to genesis and starting to rescan the blockchain"
        );
        let mut db_tx = self.db.transaction_rw(None)?;
        let mut accounts =
            Self::reset_wallet_transactions_and_load(self.chain_config.clone(), &mut db_tx)?;
        self.next_unused_account = accounts.pop_last().expect("not empty accounts");
        self.accounts = accounts;
        db_tx.commit()?;
        Ok(())
    }

    fn reset_wallet_transactions(
        chain_config: Arc<ChainConfig>,
        db_tx: &mut impl WalletStorageWriteLocked,
    ) -> WalletResult<()> {
        db_tx.clear_transactions()?;
        db_tx.clear_addresses()?;
        db_tx.clear_public_keys()?;

        let lookahead_size = db_tx.get_lookahead_size().unwrap_or(LOOKAHEAD_SIZE);

        // set all accounts best block to genesis
        for (id, mut info) in db_tx.get_accounts_info()? {
            info.update_best_block(BlockHeight::new(0), chain_config.genesis_block_id());
            info.set_lookahead_size(lookahead_size);
            db_tx.set_account(&id, &info)?;
            db_tx.set_account_unconfirmed_tx_counter(&id, 0)?;
            db_tx.set_keychain_usage_state(
                &AccountKeyPurposeId::new(id.clone(), KeyPurpose::Change),
                &KeychainUsageState::new(None, None),
            )?;
            db_tx.set_keychain_usage_state(
                &AccountKeyPurposeId::new(id.clone(), KeyPurpose::ReceiveFunds),
                &KeychainUsageState::new(None, None),
            )?;
            db_tx
                .set_vrf_keychain_usage_state(&id.clone(), &KeychainUsageState::new(None, None))?;
        }

        Ok(())
    }

    fn reset_wallet_transactions_and_load(
        chain_config: Arc<ChainConfig>,
        db_tx: &mut impl WalletStorageWriteLocked,
    ) -> WalletResult<BTreeMap<U31, Account>> {
        Self::reset_wallet_transactions(chain_config.clone(), db_tx)?;

        // set all accounts best block to genesis
        db_tx
            .get_accounts_info()?
            .into_keys()
            .map(|id| {
                let mut account = Account::load_from_database(chain_config.clone(), db_tx, &id)?;
                account.top_up_addresses(db_tx)?;
                account.scan_genesis(db_tx, &WalletEventsNoOp)?;

                Ok((account.account_index(), account))
            })
            .collect()
    }

    fn migrate_next_unused_account(
        chain_config: Arc<ChainConfig>,
        db_tx: &mut impl WalletStorageWriteUnlocked,
    ) -> Result<(), WalletError> {
        let key_chain = MasterKeyChain::new_from_existing_database(chain_config.clone(), db_tx)?;
        let accounts_info = db_tx.get_accounts_info()?;
        let mut accounts: BTreeMap<U31, Account> = accounts_info
            .keys()
            .map(|account_id| Account::load_from_database(chain_config.clone(), db_tx, account_id))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|account| (account.account_index(), account))
            .collect();
        let last_account = accounts.pop_last().ok_or(WalletError::WalletNotInitialized)?;
        let next_account_index = last_account
            .0
            .plus_one()
            .map_err(|_| WalletError::AbsoluteMaxNumAccountsExceeded(last_account.0))?;
        Wallet::<B>::create_next_unused_account(
            next_account_index,
            chain_config.clone(),
            &key_chain,
            db_tx,
            None,
        )?;
        Ok(())
    }

    pub fn load_wallet<F: Fn(u32) -> WalletResult<()>>(
        chain_config: Arc<ChainConfig>,
        mut db: Store<B>,
        password: Option<String>,
        pre_migration: F,
        wallet_type: WalletType,
        force_change_wallet_type: bool,
    ) -> WalletResult<Self> {
        if let Some(password) = password {
            db.unlock_private_keys(&password)?;
        }
        Self::check_and_migrate_db(&db, chain_config.clone(), pre_migration, wallet_type)?;
        if force_change_wallet_type {
            Self::force_migrate_wallet_type(wallet_type, &db, chain_config.clone())?;
        }

        // Please continue to use read-only transaction here.
        // Some unit tests expect that loading the wallet does not change the DB.
        let db_tx = db.transaction_ro()?;

        Self::validate_chain_info(chain_config.as_ref(), &db_tx, wallet_type)?;

        let key_chain = MasterKeyChain::new_from_existing_database(chain_config.clone(), &db_tx)?;

        let accounts_info = db_tx.get_accounts_info()?;

        let mut accounts: BTreeMap<U31, Account> = accounts_info
            .keys()
            .map(|account_id| {
                Account::load_from_database(Arc::clone(&chain_config), &db_tx, account_id)
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|account| (account.account_index(), account))
            .collect();

        let latest_median_time =
            db_tx.get_median_time()?.unwrap_or(chain_config.genesis_block().timestamp());

        db_tx.close();

        let next_unused_account = accounts.pop_last().ok_or(WalletError::WalletNotInitialized)?;

        Ok(Wallet {
            chain_config,
            db,
            key_chain,
            accounts,
            latest_median_time,
            next_unused_account,
        })
    }

    pub fn seed_phrase(&self) -> WalletResult<Option<SerializableSeedPhrase>> {
        self.db.transaction_ro_unlocked()?.get_seed_phrase().map_err(WalletError::from)
    }

    pub fn delete_seed_phrase(&self) -> WalletResult<Option<SerializableSeedPhrase>> {
        let mut tx = self.db.transaction_rw_unlocked(None)?;
        let seed_phrase = tx.del_seed_phrase().map_err(WalletError::from)?;
        tx.commit()?;

        Ok(seed_phrase)
    }

    pub fn is_encrypted(&self) -> bool {
        self.db.is_encrypted()
    }

    pub fn is_locked(&self) -> bool {
        self.db.is_locked()
    }

    pub fn encrypt_wallet(&mut self, password: &Option<String>) -> WalletResult<()> {
        self.db.encrypt_private_keys(password).map_err(WalletError::from)
    }

    pub fn lock_wallet(&mut self) -> WalletResult<()> {
        self.db.lock_private_keys().map_err(WalletError::from)
    }

    pub fn unlock_wallet(&mut self, password: &String) -> WalletResult<()> {
        self.db.unlock_private_keys(password).map_err(WalletError::from)
    }

    pub fn set_lookahead_size(
        &mut self,
        lookahead_size: u32,
        force_reduce: bool,
    ) -> WalletResult<()> {
        let last_used = self.accounts.values().fold(None, |last, acc| {
            let usage = acc.get_addresses_usage();
            std::cmp::max(last, usage.last_used().map(U31::into_u32))
        });

        if let Some(last_used) = last_used {
            ensure!(
                last_used < lookahead_size || force_reduce,
                WalletError::ReducedLookaheadSize(lookahead_size, last_used)
            );
        }

        let mut db_tx = self.db.transaction_rw(None)?;
        db_tx.set_lookahead_size(lookahead_size)?;
        let mut accounts =
            Self::reset_wallet_transactions_and_load(self.chain_config.clone(), &mut db_tx)?;
        self.next_unused_account = accounts.pop_last().expect("not empty accounts");
        self.accounts = accounts;
        db_tx.commit()?;

        Ok(())
    }

    pub fn account_indexes(&self) -> impl Iterator<Item = &U31> {
        self.accounts.keys()
    }

    pub fn number_of_accounts(&self) -> usize {
        self.accounts.len()
    }

    pub fn wallet_info(&self) -> (H256, Vec<Option<String>>) {
        let acc_id = self.accounts.values().next().expect("not empty").get_account_id();
        let names = self.accounts.values().map(|acc| acc.name().clone()).collect();
        (hash_encoded(&acc_id), names)
    }

    fn create_next_unused_account(
        next_account_index: U31,
        chain_config: Arc<ChainConfig>,
        master_key_chain: &MasterKeyChain,
        db_tx: &mut impl WalletStorageWriteUnlocked,
        name: Option<String>,
    ) -> WalletResult<(U31, Account)> {
        ensure!(
            name.as_ref().map_or(true, |name| !name.is_empty()),
            WalletError::EmptyAccountName
        );

        let lookahead_size = db_tx.get_lookahead_size()?;
        let account_key_chain =
            master_key_chain.create_account_key_chain(db_tx, next_account_index, lookahead_size)?;

        let account = Account::new(chain_config, db_tx, account_key_chain, name)?;

        Ok((next_account_index, account))
    }

    /// Promotes the unused account into the used accounts and creates a new unused account
    /// Returns the new index and optional name if provided
    pub fn create_next_account(
        &mut self,
        name: Option<String>,
    ) -> WalletResult<(U31, Option<String>)> {
        ensure!(
            self.accounts
                .values()
                .last()
                .expect("must have a default account")
                .has_transactions(),
            WalletError::EmptyLastAccount
        );
        ensure!(
            name.as_ref().map_or(true, |name| !name.is_empty()),
            WalletError::EmptyAccountName
        );

        let next_account_index =
            self.next_unused_account.0.plus_one().map_err(|_| {
                WalletError::AbsoluteMaxNumAccountsExceeded(self.next_unused_account.0)
            })?;

        let mut db_tx = self.db.transaction_rw_unlocked(None)?;

        let mut next_unused_account = Self::create_next_unused_account(
            next_account_index,
            self.chain_config.clone(),
            &self.key_chain,
            &mut db_tx,
            None,
        )?;

        self.next_unused_account.1.set_name(name.clone(), &mut db_tx)?;
        std::mem::swap(&mut self.next_unused_account, &mut next_unused_account);
        let (next_account_index, next_account) = next_unused_account;

        // no need to rescan the blockchain from the start for the next unused account as we have been
        // scanning for addresses of the previous next unused account and it is not allowed to create a gap in
        // the account indexes
        let (best_block_id, best_block_height) = next_account.best_block();
        self.next_unused_account.1.update_best_block(
            &mut db_tx,
            best_block_height,
            best_block_id,
        )?;

        db_tx.commit()?;

        self.accounts.insert(next_account_index, next_account);

        Ok((next_account_index, name))
    }

    pub fn set_account_name(
        &mut self,
        account_index: U31,
        name: Option<String>,
    ) -> WalletResult<(U31, Option<String>)> {
        self.for_account_rw(account_index, |acc, db_tx| {
            acc.set_name(name, db_tx).map(|()| (acc.account_index(), acc.name().clone()))
        })
    }

    pub fn database(&self) -> &Store<B> {
        &self.db
    }

    fn for_account_rw<T>(
        &mut self,
        account_index: U31,
        f: impl FnOnce(&mut Account, &mut StoreTxRw<B>) -> WalletResult<T>,
    ) -> WalletResult<T> {
        let mut db_tx = self.db.transaction_rw(None)?;
        let account = Self::get_account_mut(&mut self.accounts, account_index)?;
        let value = f(account, &mut db_tx)?;
        // The in-memory wallet state has already changed, so rolling back
        // the DB transaction will make the wallet state inconsistent.
        // This should not happen with the sqlite backend in normal situations,
        // so let's abort the process instead.
        db_tx.commit().expect("RW transaction commit failed unexpectedly");
        Ok(value)
    }

    fn for_account_rw_unlocked<T>(
        &mut self,
        account_index: U31,
        f: impl FnOnce(&mut Account, &mut StoreTxRwUnlocked<B>, &ChainConfig) -> WalletResult<T>,
    ) -> WalletResult<T> {
        let mut db_tx = self.db.transaction_rw_unlocked(None)?;
        let account = Self::get_account_mut(&mut self.accounts, account_index)?;
        match f(account, &mut db_tx, &self.chain_config) {
            Ok(value) => {
                // Abort the process if the DB transaction fails. See `for_account_rw` for more information.
                db_tx.commit().expect("RW transaction commit failed unexpectedly");
                Ok(value)
            }
            Err(err) => {
                db_tx.abort();
                // In case of an error reload the keys in case the operation issued new ones and
                // are saved in the cache but not in the DB
                let db_tx = self.db.transaction_ro()?;
                account.reload_keys(&db_tx)?;
                Err(err)
            }
        }
    }

    fn for_account_rw_unlocked_and_check_tx_custom_error(
        &mut self,
        account_index: U31,
        f: impl FnOnce(&mut Account, &mut StoreTxRwUnlocked<B>) -> WalletResult<SendRequest>,
        error_mapper: impl FnOnce(WalletError) -> WalletError,
    ) -> WalletResult<SignedTransaction> {
        let (_, block_height) = self.get_best_block_for_account(account_index)?;
        self.for_account_rw_unlocked(account_index, |account, db_tx, chain_config| {
            let request = f(account, db_tx)?;

            let ptx = request.into_partially_signed_tx()?;

            let signer = SoftwareSigner::new(db_tx, Arc::new(chain_config.clone()), account_index);
            let tx = signer
                .sign_ptx(ptx, account.key_chain())
                .map(|(ptx, _, _)| ptx)?
                .into_signed_tx(chain_config)
                .map_err(error_mapper)?;

            check_transaction(chain_config, block_height.next_height(), &tx)?;
            Ok(tx)
        })
    }

    fn for_account_rw_unlocked_and_check_tx(
        &mut self,
        account_index: U31,
        f: impl FnOnce(&mut Account, &mut StoreTxRwUnlocked<B>) -> WalletResult<SendRequest>,
    ) -> WalletResult<SignedTransaction> {
        self.for_account_rw_unlocked_and_check_tx_custom_error(account_index, f, |err| err)
    }

    fn get_account(&self, account_index: U31) -> WalletResult<&Account> {
        self.accounts
            .get(&account_index)
            .ok_or(WalletError::NoAccountFoundWithIndex(account_index))
    }

    fn get_account_mut(
        accounts: &mut BTreeMap<U31, Account>,
        account_index: U31,
    ) -> WalletResult<&mut Account> {
        accounts
            .get_mut(&account_index)
            .ok_or(WalletError::NoAccountFoundWithIndex(account_index))
    }

    pub fn get_balance(
        &self,
        account_index: U31,
        utxo_states: UtxoStates,
        with_locked: WithLocked,
    ) -> WalletResult<BTreeMap<Currency, Amount>> {
        self.get_account(account_index)?.get_balance(
            utxo_states,
            self.latest_median_time,
            with_locked,
        )
    }

    pub fn get_multisig_utxos(
        &self,
        account_index: U31,
        utxo_types: UtxoTypes,
        utxo_states: UtxoStates,
        with_locked: WithLocked,
    ) -> WalletResult<Vec<(UtxoOutPoint, TxOutput, Option<TokenId>)>> {
        let account = self.get_account(account_index)?;
        let utxos = account.get_multisig_utxos(
            utxo_types,
            self.latest_median_time,
            utxo_states,
            with_locked,
        );
        let utxos = utxos
            .into_iter()
            .map(|(outpoint, (txo, token_id))| (outpoint, txo.clone(), token_id))
            .collect();
        Ok(utxos)
    }

    pub fn get_utxos(
        &self,
        account_index: U31,
        utxo_types: UtxoTypes,
        utxo_states: UtxoStates,
        with_locked: WithLocked,
    ) -> WalletResult<Vec<(UtxoOutPoint, TxOutput, Option<TokenId>)>> {
        let account = self.get_account(account_index)?;
        let utxos = account.get_utxos(
            utxo_types,
            self.latest_median_time,
            utxo_states,
            with_locked,
        );
        let utxos = utxos
            .into_iter()
            .map(|(outpoint, (txo, token_id))| (outpoint, txo.clone(), token_id))
            .collect();
        Ok(utxos)
    }

    pub fn find_unspent_utxo_with_destination(
        &self,
        outpoint: &UtxoOutPoint,
    ) -> Option<(TxOutput, Destination)> {
        self.accounts.values().find_map(|acc: &Account| {
            let current_block_info = BlockInfo {
                height: acc.best_block().1,
                timestamp: self.latest_median_time,
            };
            acc.find_unspent_utxo_with_destination(outpoint, current_block_info).ok()
        })
    }

    pub fn pending_transactions(
        &self,
        account_index: U31,
    ) -> WalletResult<Vec<WithId<&Transaction>>> {
        let account = self.get_account(account_index)?;
        let transactions = account.pending_transactions();
        Ok(transactions)
    }

    pub fn mainchain_transactions(
        &self,
        account_index: U31,
        destination: Option<Destination>,
        limit: usize,
    ) -> WalletResult<Vec<TxInfo>> {
        let account = self.get_account(account_index)?;
        let transactions = account.mainchain_transactions(destination, limit);
        Ok(transactions)
    }

    pub fn abandon_transaction(
        &mut self,
        account_index: U31,
        tx_id: Id<Transaction>,
    ) -> WalletResult<()> {
        self.for_account_rw(account_index, |account, db_tx| {
            account.abandon_transaction(tx_id, db_tx)
        })
    }

    pub fn get_pool_ids(
        &self,
        account_index: U31,
        filter: WalletPoolsFilter,
    ) -> WalletResult<Vec<(PoolId, PoolData)>> {
        let db_tx = self.db.transaction_ro_unlocked()?;
        let pool_ids = self.get_account(account_index)?.get_pool_ids(filter, &db_tx);
        Ok(pool_ids)
    }

    pub fn get_delegations(
        &self,
        account_index: U31,
    ) -> WalletResult<impl Iterator<Item = (&DelegationId, &DelegationData)>> {
        let delegations = self.get_account(account_index)?.get_delegations();
        Ok(delegations)
    }

    pub fn get_delegation(
        &self,
        account_index: U31,
        delegation_id: DelegationId,
    ) -> WalletResult<&DelegationData> {
        self.get_account(account_index)?.find_delegation(&delegation_id)
    }

    pub fn get_created_blocks(
        &self,
        account_index: U31,
    ) -> WalletResult<Vec<(BlockHeight, Id<GenBlock>, PoolId)>> {
        let block_ids = self.get_account(account_index)?.get_created_blocks();
        Ok(block_ids)
    }

    pub fn standalone_address_label_rename(
        &mut self,
        account_index: U31,
        address: Destination,
        label: Option<String>,
    ) -> WalletResult<()> {
        self.for_account_rw(account_index, |account, db_tx| {
            account.standalone_address_label_rename(db_tx, address, label)
        })
    }

    pub fn add_standalone_address(
        &mut self,
        account_index: U31,
        public_key_hash: PublicKeyHash,
        label: Option<String>,
    ) -> WalletResult<()> {
        self.for_account_rw(account_index, |account, db_tx| {
            account.add_standalone_address(db_tx, public_key_hash, label)
        })
    }

    pub fn add_standalone_private_key(
        &mut self,
        account_index: U31,
        private_key: PrivateKey,
        label: Option<String>,
    ) -> WalletResult<()> {
        self.for_account_rw_unlocked(account_index, |account, db_tx, _| {
            account.add_standalone_private_key(db_tx, private_key, label)
        })
    }

    pub fn add_standalone_multisig(
        &mut self,
        account_index: U31,
        challenge: ClassicMultisigChallenge,
        label: Option<String>,
    ) -> WalletResult<PublicKeyHash> {
        self.for_account_rw(account_index, |account, db_tx| {
            account.add_standalone_multisig(db_tx, challenge, label)
        })
    }

    pub fn get_new_address(
        &mut self,
        account_index: U31,
    ) -> WalletResult<(ChildNumber, Address<Destination>)> {
        self.for_account_rw(account_index, |account, db_tx| {
            account.get_new_address(db_tx, KeyPurpose::ReceiveFunds)
        })
    }

    pub fn get_vrf_key(
        &mut self,
        account_index: U31,
    ) -> WalletResult<(ChildNumber, Address<VRFPublicKey>)> {
        self.for_account_rw(account_index, |account, db_tx| {
            account.get_new_vrf_key(db_tx)
        })
    }

    pub fn find_public_key(
        &mut self,
        account_index: U31,
        address: Destination,
    ) -> WalletResult<PublicKey> {
        let account = self.get_account(account_index)?;
        match address {
            Destination::PublicKeyHash(addr) => account.find_corresponding_pub_key(&addr),
            Destination::PublicKey(pk) => Ok(pk),
            Destination::ScriptHash(_)
            | Destination::AnyoneCanSpend
            | Destination::ClassicMultisig(_) => Err(WalletError::NoUtxos),
        }
    }

    pub fn get_transaction_list(
        &self,
        account_index: U31,
        skip: usize,
        count: usize,
    ) -> WalletResult<TransactionList> {
        let account = self.get_account(account_index)?;
        account.get_transaction_list(skip, count)
    }

    pub fn get_transaction(
        &self,
        account_index: U31,
        transaction_id: Id<Transaction>,
    ) -> WalletResult<&TxData> {
        let account = self.get_account(account_index)?;
        account.get_transaction(transaction_id)
    }

    pub fn get_transactions_to_be_broadcast(&self) -> WalletResult<Vec<SignedTransaction>> {
        self.db
            .transaction_ro()?
            .get_user_transactions()
            .map_err(WalletError::DatabaseError)
    }

    pub fn get_all_issued_addresses(
        &self,
        account_index: U31,
    ) -> WalletResult<BTreeMap<ChildNumber, Address<Destination>>> {
        let account = self.get_account(account_index)?;
        Ok(account.get_all_issued_addresses())
    }

    pub fn get_all_standalone_addresses(
        &self,
        account_index: U31,
    ) -> WalletResult<StandaloneAddresses> {
        let account = self.get_account(account_index)?;
        Ok(account.get_all_standalone_addresses())
    }

    pub fn get_all_standalone_address_details(
        &self,
        account_index: U31,
        address: Destination,
    ) -> WalletResult<(
        Destination,
        BTreeMap<Currency, Amount>,
        StandaloneAddressDetails,
    )> {
        let account = self.get_account(account_index)?;
        account.get_all_standalone_address_details(address, self.latest_median_time)
    }

    pub fn get_all_issued_vrf_public_keys(
        &self,
        account_index: U31,
    ) -> WalletResult<BTreeMap<ChildNumber, (Address<VRFPublicKey>, bool)>> {
        let account = self.get_account(account_index)?;
        Ok(account.get_all_issued_vrf_public_keys())
    }

    pub fn get_legacy_vrf_public_key(
        &self,
        account_index: U31,
    ) -> WalletResult<Address<VRFPublicKey>> {
        let account = self.get_account(account_index)?;
        Ok(account.get_legacy_vrf_public_key())
    }

    pub fn get_addresses_usage(&self, account_index: U31) -> WalletResult<&KeychainUsageState> {
        let account = self.get_account(account_index)?;
        Ok(account.get_addresses_usage())
    }

    /// Creates a transaction to send funds to specified addresses.
    ///
    /// # Arguments
    ///
    /// * `&mut self` - A mutable reference to the wallet instance.
    /// * `account_index: U31` - The index of the account from which funds will be sent.
    /// * `outputs: impl IntoIterator<Item = TxOutput>` - An iterator over `TxOutput` items representing the addresses and amounts to which funds will be sent.
    /// * `inputs`: SelectedInputs - if not empty will try to select inputs from those instead of the available ones
    /// * `change_addresses`: if present will use those change_addresses instead of generating new ones
    /// * `current_fee_rate: FeeRate` - The current fee rate based on the mempool to be used for the transaction.
    /// * `consolidate_fee_rate: FeeRate` - The fee rate in case of a consolidation event, if the
    /// current_fee_rate is lower than the consolidate_fee_rate then the wallet will tend to
    /// use and consolidate multiple smaller inputs, else if the current_fee_rate is higher it will
    /// tend to use inputs with lowest fee.
    ///
    /// # Returns
    ///
    /// A `WalletResult` containing the signed transaction if successful, or an error indicating the reason for failure.
    pub fn create_transaction_to_addresses(
        &mut self,
        account_index: U31,
        outputs: impl IntoIterator<Item = TxOutput>,
        inputs: SelectedInputs,
        change_addresses: BTreeMap<Currency, Address<Destination>>,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let request = SendRequest::new().with_outputs(outputs);
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.process_send_request_and_sign(
                db_tx,
                request,
                inputs,
                change_addresses,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn create_unsigned_transaction_to_addresses(
        &mut self,
        account_index: U31,
        outputs: impl IntoIterator<Item = TxOutput>,
        inputs: SelectedInputs,
        change_addresses: BTreeMap<Currency, Address<Destination>>,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<(PartiallySignedTransaction, BTreeMap<Currency, Amount>)> {
        let request = SendRequest::new().with_outputs(outputs);
        let latest_median_time = self.latest_median_time;
        self.for_account_rw(account_index, |account, db_tx| {
            account.process_send_request(
                db_tx,
                request,
                inputs,
                change_addresses,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn create_sweep_transaction(
        &mut self,
        account_index: U31,
        destination: Destination,
        inputs: Vec<(UtxoOutPoint, TxOutput, Option<TokenId>)>,
        current_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let request = SendRequest::new().with_inputs(
            inputs
                .into_iter()
                .map(|(outpoint, output, _)| (TxInput::Utxo(outpoint), output)),
            &|_| None,
        )?;

        self.for_account_rw_unlocked_and_check_tx(account_index, |account, _| {
            account.sweep_addresses(destination, request, current_fee_rate)
        })
    }

    pub fn create_sweep_from_delegation_transaction(
        &mut self,
        account_index: U31,
        address: Address<Destination>,
        delegation_id: DelegationId,
        delegation_share: Amount,
        current_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, _| {
            account.sweep_delegation(address, delegation_id, delegation_share, current_fee_rate)
        })
    }

    pub fn create_transaction_to_addresses_from_delegation(
        &mut self,
        account_index: U31,
        address: Address<Destination>,
        amount: Amount,
        delegation_id: DelegationId,
        delegation_share: Amount,
        current_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, _| {
            account.spend_from_delegation(
                address,
                amount,
                delegation_id,
                delegation_share,
                current_fee_rate,
            )
        })
    }

    pub fn mint_tokens(
        &mut self,
        account_index: U31,
        token_info: &UnconfirmedTokenInfo,
        amount: Amount,
        destination: Address<Destination>,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.mint_tokens(
                db_tx,
                token_info,
                destination,
                amount,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn unmint_tokens(
        &mut self,
        account_index: U31,
        token_info: &UnconfirmedTokenInfo,
        amount: Amount,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.unmint_tokens(
                db_tx,
                token_info,
                amount,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn lock_token_supply(
        &mut self,
        account_index: U31,
        token_info: &UnconfirmedTokenInfo,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.lock_token_supply(
                db_tx,
                token_info,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn freeze_token(
        &mut self,
        account_index: U31,
        token_info: &UnconfirmedTokenInfo,
        is_token_unfreezable: IsTokenUnfreezable,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.freeze_token(
                db_tx,
                token_info,
                is_token_unfreezable,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn unfreeze_token(
        &mut self,
        account_index: U31,
        token_info: &UnconfirmedTokenInfo,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.unfreeze_token(
                db_tx,
                token_info,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn change_token_authority(
        &mut self,
        account_index: U31,
        token_info: &UnconfirmedTokenInfo,
        address: Address<Destination>,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.change_token_authority(
                db_tx,
                token_info,
                address,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn find_used_tokens(
        &self,
        account_index: U31,
        input_utxos: &[UtxoOutPoint],
    ) -> WalletResult<BTreeSet<TokenId>> {
        self.get_account(account_index)?
            .find_used_tokens(input_utxos, self.latest_median_time)
    }

    pub fn get_token_unconfirmed_info(
        &self,
        account_index: U31,
        token_info: &RPCFungibleTokenInfo,
    ) -> WalletResult<UnconfirmedTokenInfo> {
        self.get_account(account_index)?.get_token_unconfirmed_info(token_info)
    }

    pub fn create_delegation(
        &mut self,
        account_index: U31,
        outputs: Vec<TxOutput>,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<(DelegationId, SignedTransaction)> {
        let tx = self.create_transaction_to_addresses(
            account_index,
            outputs,
            SelectedInputs::Utxos(vec![]),
            BTreeMap::new(),
            current_fee_rate,
            consolidate_fee_rate,
        )?;
        let input0_outpoint = tx
            .transaction()
            .inputs()
            .first()
            .ok_or(WalletError::NoUtxos)?
            .utxo_outpoint()
            .ok_or(WalletError::NoUtxos)?;
        let delegation_id = make_delegation_id(input0_outpoint);
        Ok((delegation_id, tx))
    }

    pub fn issue_new_token(
        &mut self,
        account_index: U31,
        token_issuance: TokenIssuance,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<(TokenId, SignedTransaction)> {
        let outputs = make_issue_token_outputs(token_issuance, self.chain_config.as_ref())?;

        let tx = self.create_transaction_to_addresses(
            account_index,
            outputs,
            SelectedInputs::Utxos(vec![]),
            BTreeMap::new(),
            current_fee_rate,
            consolidate_fee_rate,
        )?;
        let token_id =
            make_token_id(tx.transaction().inputs()).ok_or(WalletError::MissingTokenId)?;
        Ok((token_id, tx))
    }

    pub fn issue_new_nft(
        &mut self,
        account_index: U31,
        address: Address<Destination>,
        metadata: Metadata,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
    ) -> WalletResult<(TokenId, SignedTransaction)> {
        let destination = address.into_object();
        let latest_median_time = self.latest_median_time;

        let signed_transaction =
            self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
                account.create_issue_nft_tx(
                    db_tx,
                    IssueNftArguments {
                        metadata,
                        destination,
                    },
                    latest_median_time,
                    CurrentFeeRate {
                        current_fee_rate,
                        consolidate_fee_rate,
                    },
                )
            })?;

        let token_id = make_token_id(signed_transaction.transaction().inputs())
            .ok_or(WalletError::MissingTokenId)?;
        Ok((token_id, signed_transaction))
    }

    pub fn create_stake_pool_tx(
        &mut self,
        account_index: U31,
        current_fee_rate: FeeRate,
        consolidate_fee_rate: FeeRate,
        stake_pool_arguments: StakePoolDataArguments,
    ) -> WalletResult<SignedTransaction> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked_and_check_tx(account_index, |account, db_tx| {
            account.create_stake_pool_tx(
                db_tx,
                stake_pool_arguments,
                latest_median_time,
                CurrentFeeRate {
                    current_fee_rate,
                    consolidate_fee_rate,
                },
            )
        })
    }

    pub fn decommission_stake_pool(
        &mut self,
        account_index: U31,
        pool_id: PoolId,
        pool_balance: Amount,
        output_address: Option<Destination>,
        current_fee_rate: FeeRate,
    ) -> WalletResult<SignedTransaction> {
        self.for_account_rw_unlocked_and_check_tx_custom_error(
            account_index,
            |account, db_tx| {
                account.decommission_stake_pool(
                    db_tx,
                    pool_id,
                    pool_balance,
                    output_address,
                    current_fee_rate,
                )
            },
            |_err| WalletError::PartiallySignedTransactionInDecommissionCommand,
        )
    }

    pub fn decommission_stake_pool_request(
        &mut self,
        account_index: U31,
        pool_id: PoolId,
        pool_balance: Amount,
        output_address: Option<Destination>,
        current_fee_rate: FeeRate,
    ) -> WalletResult<PartiallySignedTransaction> {
        self.for_account_rw_unlocked(account_index, |account, db_tx, chain_config| {
            let request = account.decommission_stake_pool_request(
                db_tx,
                pool_id,
                pool_balance,
                output_address,
                current_fee_rate,
            )?;

            let ptx = request.into_partially_signed_tx()?;

            let signer = SoftwareSigner::new(db_tx, Arc::new(chain_config.clone()), account_index);
            let ptx = signer.sign_ptx(ptx, account.key_chain()).map(|(ptx, _, _)| ptx)?;

            if ptx.is_fully_signed(chain_config) {
                return Err(WalletError::FullySignedTransactionInDecommissionReq);
            }
            Ok(ptx)
        })
    }

    pub fn sign_raw_transaction(
        &mut self,
        account_index: U31,
        tx: TransactionToSign,
    ) -> WalletResult<(
        PartiallySignedTransaction,
        Vec<SignatureStatus>,
        Vec<SignatureStatus>,
    )> {
        let latest_median_time = self.latest_median_time;
        self.for_account_rw_unlocked(account_index, |account, db_tx, chain_config| {
            let ptx = match tx {
                TransactionToSign::Partial(ptx) => ptx,
                TransactionToSign::Tx(tx) => {
                    account.tx_to_partially_signed_tx(tx, latest_median_time)?
                }
            };
            let signer = SoftwareSigner::new(db_tx, Arc::new(chain_config.clone()), account_index);

            let res = signer.sign_ptx(ptx, account.key_chain())?;
            Ok(res)
        })
    }

    pub fn sign_challenge(
        &mut self,
        account_index: U31,
        challenge: Vec<u8>,
        destination: Destination,
    ) -> WalletResult<ArbitraryMessageSignature> {
        self.for_account_rw_unlocked(account_index, |account, db_tx, chain_config| {
            let signer = SoftwareSigner::new(db_tx, Arc::new(chain_config.clone()), account_index);
            let msg = signer.sign_challenge(challenge, destination, account.key_chain())?;
            Ok(msg)
        })
    }

    pub fn get_pos_gen_block_data(
        &self,
        account_index: U31,
        pool_id: PoolId,
    ) -> WalletResult<PoSGenerateBlockInputData> {
        let db_tx = self.db.transaction_ro_unlocked()?;
        self.get_account(account_index)?.get_pos_gen_block_data(&db_tx, pool_id)
    }

    pub fn get_pos_gen_block_data_by_pool_id(
        &self,
        pool_id: PoolId,
    ) -> WalletResult<PoSGenerateBlockInputData> {
        let db_tx = self.db.transaction_ro_unlocked()?;

        for acc in self.accounts.values() {
            if acc.pool_exists(pool_id) {
                return acc.get_pos_gen_block_data(&db_tx, pool_id);
            }
        }

        Err(WalletError::UnknownPoolId(pool_id))
    }

    /// Returns the last scanned block hash and height for all accounts.
    /// Returns genesis block when the wallet is just created.
    pub fn get_best_block(&self) -> BTreeMap<U31, (Id<GenBlock>, BlockHeight)> {
        self.accounts
            .iter()
            .map(|(index, account)| (*index, account.best_block()))
            .collect()
    }

    /// Returns the last scanned block hash and height for the account.
    /// Returns genesis block when the account is just created.
    pub fn get_best_block_for_account(
        &self,
        account_index: U31,
    ) -> WalletResult<(Id<GenBlock>, BlockHeight)> {
        Ok(self.get_account(account_index)?.best_block())
    }

    /// Returns the syncing state of the wallet
    /// includes the last scanned block hash and height for each account and the next unused one
    /// if in syncing state else NewlyCreated if this is the first sync after creating a new wallet
    pub fn get_syncing_state(&self) -> WalletSyncingState {
        WalletSyncingState {
            account_best_blocks: self.get_best_block(),
            unused_account_best_block: self.next_unused_account.1.best_block(),
        }
    }

    /// Scan new blocks and update best block hash/height.
    /// New block may reset the chain of previously scanned blocks.
    ///
    /// `common_block_height` is the height of the shared blocks that are still in sync after reorgs.
    /// If `common_block_height` is zero, only the genesis block is considered common.
    pub fn scan_new_blocks(
        &mut self,
        account_index: U31,
        common_block_height: BlockHeight,
        blocks: Vec<Block>,
        wallet_events: &impl WalletEvents,
    ) -> WalletResult<()> {
        self.for_account_rw(account_index, |acc, db_tx| {
            acc.scan_new_blocks(db_tx, wallet_events, common_block_height, &blocks)
        })?;

        wallet_events.new_block();
        Ok(())
    }

    /// Scan new blocks and update best block hash/height.
    /// New block may reset the chain of previously scanned blocks.
    ///
    /// `common_block_height` is the height of the shared blocks that are still in sync after reorgs.
    /// If `common_block_height` is zero, only the genesis block is considered common.
    /// If a new transaction is recognized for the unused account, it is transferred to the used
    /// accounts and a new unused account is created.
    pub fn scan_new_blocks_unused_account(
        &mut self,
        common_block_height: BlockHeight,
        blocks: Vec<Block>,
        wallet_events: &impl WalletEvents,
    ) -> WalletResult<()> {
        loop {
            let mut db_tx = self.db.transaction_rw(None)?;
            let added_new_tx_in_unused_acc = self.next_unused_account.1.scan_new_blocks(
                &mut db_tx,
                wallet_events,
                common_block_height,
                &blocks,
            )?;

            db_tx.commit()?;

            if added_new_tx_in_unused_acc {
                self.create_next_account(None)?;
            } else {
                break;
            }
        }

        wallet_events.new_block();
        Ok(())
    }

    /// Sets the best block for all accounts
    /// Should be called after creating a new wallet
    fn set_best_block(
        &mut self,
        best_block_height: BlockHeight,
        best_block_id: Id<GenBlock>,
    ) -> WalletResult<()> {
        let mut db_tx = self.db.transaction_rw(None)?;

        for account in self.accounts.values_mut() {
            account.update_best_block(&mut db_tx, best_block_height, best_block_id)?;
        }

        self.next_unused_account.1.update_best_block(
            &mut db_tx,
            best_block_height,
            best_block_id,
        )?;

        db_tx.commit()?;

        Ok(())
    }

    /// Rescan mempool for unconfirmed transactions and UTXOs
    /// TODO: Currently we don't sync with the mempool
    #[cfg(test)]
    pub fn scan_mempool(
        &mut self,
        transactions: &[SignedTransaction],
        wallet_events: &impl WalletEvents,
    ) -> WalletResult<()> {
        let mut db_tx = self.db.transaction_rw(None)?;

        for account in self.accounts.values_mut() {
            account.scan_new_inmempool_transactions(transactions, &mut db_tx, wallet_events)?;
        }

        db_tx.commit()?;

        Ok(())
    }

    /// Save an unconfirmed transaction in case we need to rebroadcast it later
    /// and mark it as Inactive for now
    pub fn add_unconfirmed_tx(
        &mut self,
        transaction: SignedTransaction,
        wallet_events: &impl WalletEvents,
    ) -> WalletResult<()> {
        let mut db_tx = self.db.transaction_rw(None)?;

        let txs = [transaction];
        for account in self.accounts.values_mut() {
            account.scan_new_inactive_transactions(&txs, &mut db_tx, wallet_events)?;
        }

        db_tx.commit()?;

        Ok(())
    }

    /// Save an unconfirmed transaction for a specific account in case we need to rebroadcast it later
    /// and mark it as Inactive for now
    pub fn add_account_unconfirmed_tx(
        &mut self,
        account_index: U31,
        transaction: SignedTransaction,
        wallet_events: &impl WalletEvents,
    ) -> WalletResult<()> {
        self.for_account_rw(account_index, |acc, db_tx| {
            acc.scan_new_inactive_transactions(&[transaction], db_tx, wallet_events)
        })
    }

    pub fn set_median_time(&mut self, median_time: BlockTimestamp) -> WalletResult<()> {
        self.latest_median_time = median_time;
        let mut db_tx = self.db.transaction_rw(None)?;
        db_tx.set_median_time(median_time)?;
        db_tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
