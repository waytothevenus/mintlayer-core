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

use crate::{
    api::json_helpers::{
        amount_to_json, block_header_to_json, to_tx_json_with_block_info, tx_to_json,
        txoutput_to_json, utxo_outpoint_to_json, TokenDecimals,
    },
    error::{
        ApiServerWebServerClientError, ApiServerWebServerError, ApiServerWebServerForbiddenError,
        ApiServerWebServerNotFoundError, ApiServerWebServerServerError,
    },
    TxSubmitClient,
};
use api_server_common::storage::storage_api::{
    block_aux_data::BlockAuxData, ApiServerStorage, ApiServerStorageRead, BlockInfo,
    TransactionInfo,
};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use common::{
    address::Address,
    chain::{
        block::timestamp::BlockTimestamp,
        tokens::{IsTokenFreezable, IsTokenFrozen, IsTokenUnfreezable, NftIssuance},
        Block, Destination, SignedTransaction, Transaction,
    },
    primitives::{Amount, BlockHeight, CoinOrTokenId, Id, Idable, H256},
};
use hex::ToHex;
use serde::Deserialize;
use serde_json::json;
use serialization::hex_encoded::HexEncoded;
use std::{collections::BTreeMap, ops::Sub, str::FromStr, sync::Arc, time::Duration};
use utils::ensure;

use crate::ApiServerWebServerState;

use super::json_helpers::to_json_string;

pub const API_VERSION: &str = "2.0.0";

const TX_BODY_LIMIT: usize = 10240;

pub fn routes<
    T: ApiServerStorage + Send + Sync + 'static,
    R: TxSubmitClient + Send + Sync + 'static,
>(
    enable_post_routes: bool,
) -> Router<ApiServerWebServerState<Arc<T>, Arc<R>>> {
    let router = Router::new();

    let router = router
        .route("/chain/genesis", get(chain_genesis))
        .route("/chain/tip", get(chain_tip))
        .route("/chain/:height", get(chain_at_height));

    let router = router
        .route("/block/:id", get(block))
        .route("/block/:id/header", get(block_header))
        .route("/block/:id/reward", get(block_reward))
        .route("/block/:id/transaction-ids", get(block_transaction_ids));

    let router = if enable_post_routes {
        router.route(
            "/transaction",
            post(submit_transaction).layer(DefaultBodyLimit::max(TX_BODY_LIMIT)),
        )
    } else {
        router.route("/transaction", post(forbidden_request))
    };

    let router = router.route("/feerate", get(feerate));

    let router = router
        .route("/transaction", get(transactions))
        .route("/transaction/:id", get(transaction))
        .route("/transaction/:id/merkle-path", get(transaction_merkle_path));

    let router = router
        .route("/address/:address", get(address))
        .route("/address/:address/all-utxos", get(all_address_utxos))
        .route("/address/:address/spendable-utxos", get(address_utxos))
        .route("/address/:address/delegations", get(address_delegations));

    let router = router
        .route("/pool", get(pools))
        .route("/pool/:id", get(pool))
        .route("/pool/:id/block-stats", get(pool_block_stats))
        .route("/pool/:id/delegations", get(pool_delegations));

    let router = router.route("/delegation/:id", get(delegation));

    router.route("/token/:id", get(token)).route("/nft/:id", get(nft))
}

async fn forbidden_request() -> Result<(), ApiServerWebServerError> {
    Err(ApiServerWebServerForbiddenError::Forbidden)?
}

//
// block/
//

async fn get_block(
    block_id: &str,
    state: &ApiServerWebServerState<Arc<impl ApiServerStorage>, Arc<impl TxSubmitClient>>,
) -> Result<BlockInfo, ApiServerWebServerError> {
    let block_id: Id<Block> = H256::from_str(block_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidBlockId)
        })?
        .into();

    state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_block(block_id)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .ok_or(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::BlockNotFound,
        ))
}

#[allow(clippy::unused_async)]
pub async fn block<T: ApiServerStorage>(
    Path(block_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let BlockInfo { block, height } = get_block(&block_id, &state).await?;

    Ok(Json(json!({
    "height": height,
    "header": block_header_to_json(&block.block),
    "body": {
        "reward": block.block.block_reward()
            .outputs()
            .iter()
            .map(|out| txoutput_to_json(out, &state.chain_config, &TokenDecimals::Single(None)))
            .collect::<Vec<_>>(),
        "transactions": block.block.transactions()
                            .iter()
                            .zip(block.tx_additional_infos.iter())
                            .map(|(tx, additinal_info)| tx_to_json(tx.transaction(), additinal_info, &state.chain_config))
                            .collect::<Vec<_>>(),
    },
    })))
}

#[allow(clippy::unused_async)]
pub async fn block_header<T: ApiServerStorage>(
    Path(block_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let block = get_block(&block_id, &state).await?.block;

    Ok(Json(block_header_to_json(&block.block)))
}

#[allow(clippy::unused_async)]
pub async fn block_reward<T: ApiServerStorage>(
    Path(block_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let block = get_block(&block_id, &state).await?.block;

    Ok(Json(json!(block
        .block
        .block_reward()
        .outputs()
        .iter()
        .map(|out| txoutput_to_json(out, &state.chain_config, &TokenDecimals::Single(None)))
        .collect::<Vec<_>>())))
}

pub async fn block_transaction_ids<T: ApiServerStorage>(
    Path(block_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let block = get_block(&block_id, &state).await?.block;

    let transaction_ids = block
        .block
        .transactions()
        .iter()
        .map(|tx| tx.transaction().get_id())
        .collect::<Vec<_>>();

    Ok(Json(json!(transaction_ids)))
}

//
// chain/
//

#[allow(clippy::unused_async)]
pub async fn chain_genesis<T: ApiServerStorage>(
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let genesis = state.chain_config.genesis_block();

    Ok(Json(json!({
        "block_id": genesis.get_id(),
        "genesis_message": genesis.genesis_message(),
        "timestamp": genesis.timestamp(),
        "utxos": genesis.utxos()
                 .iter()
                 .map(|out| txoutput_to_json(out, &state.chain_config, &TokenDecimals::Single(None)))
                 .collect::<Vec<_>>(),
    })))
}

pub async fn chain_at_height<T: ApiServerStorage>(
    Path(block_height): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let block_height = block_height.parse::<BlockHeight>().map_err(|_| {
        ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidBlockHeight)
    })?;

    let block_id = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_main_chain_block_id(block_height)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    match block_id {
        Some(block_id) => Ok(Json(block_id)),
        None => Err(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::NoBlockAtHeight,
        )),
    }
}

pub async fn chain_tip<T: ApiServerStorage>(
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let best_block = best_block(&state).await?;

    Ok(Json(json!({
      "block_height": best_block.block_height(),
      "block_id": best_block.block_id().to_hash().encode_hex::<String>(),
    })))
}

async fn best_block<T: ApiServerStorage>(
    state: &ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>,
) -> Result<BlockAuxData, ApiServerWebServerError> {
    state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_best_block()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })
}

//
// transaction/
//

async fn get_transaction(
    transaction_id: &str,
    state: &ApiServerWebServerState<Arc<impl ApiServerStorage>, Arc<impl TxSubmitClient>>,
) -> Result<(Option<BlockAuxData>, TransactionInfo), ApiServerWebServerError> {
    let transaction_id: Id<Transaction> = H256::from_str(transaction_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(
                ApiServerWebServerClientError::InvalidTransactionId,
            )
        })?
        .into();

    state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_transaction_with_block(transaction_id)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .ok_or(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::TransactionNotFound,
        ))
}

pub async fn feerate<T: ApiServerStorage>(
    Query(params): Query<BTreeMap<String, String>>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    const REFRESH_INTERVAL_SEC: Duration = Duration::from_secs(30);
    const IN_TOP_X_MB: &str = "in_top_x_mb";
    const DEFAULT_IN_TOP_X_MB: usize = 5;
    let in_top_x_mb = params
        .get(IN_TOP_X_MB)
        .map(|str| usize::from_str(str))
        .transpose()
        .map_err(|_| ApiServerWebServerClientError::InvalidInTopX)?
        .unwrap_or(DEFAULT_IN_TOP_X_MB);

    let feerate_points = &state.cached_values.feerate_points;

    let feerate_points: BTreeMap<_, _> = {
        let current_time = state.time_getter.get_time();
        let last_cache_time = feerate_points.read().expect("should not fail normally").0;

        if (last_cache_time + REFRESH_INTERVAL_SEC).expect("no overflow") < current_time {
            let new_feerate_points = {
                state.rpc.get_feerate_points().await.map_err(|e| {
                    logging::log::error!("internal error: {e}");
                    ApiServerWebServerError::ServerError(
                        ApiServerWebServerServerError::InternalServerError,
                    )
                })?
            };

            let mut guard = feerate_points.write().expect("should not fail normally");
            guard.0 = current_time;
            guard.1 = new_feerate_points;
            guard.1.iter().map(|(size, feerate)| (*size, *feerate)).collect()
        } else {
            let guard = feerate_points.read().expect("should not fail normally");
            guard.1.iter().map(|(size, feerate)| (*size, *feerate)).collect()
        }
    };

    let (min_size, max_feerate) = feerate_points.first_key_value().expect("not empty");
    let (max_size, min_feerate) = feerate_points.last_key_value().expect("not empty");
    let feerate = if in_top_x_mb <= *min_size {
        *max_feerate
    } else if in_top_x_mb >= *max_size {
        *min_feerate
    } else {
        mempool::find_interpolated_value(&feerate_points, in_top_x_mb).ok_or_else(|| {
            logging::log::error!(
                "internal error: could not calculate feerate {in_top_x_mb} {feerate_points:?}"
            );
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
    };

    Ok(Json(
        serde_json::to_value(feerate.atoms_per_kb().to_string()).expect("should not fail"),
    ))
}

pub async fn submit_transaction<T: ApiServerStorage>(
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
    body: String,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let tx = HexEncoded::<SignedTransaction>::from_str(&body)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(
                ApiServerWebServerClientError::InvalidSignedTransaction,
            )
        })?
        .take();

    let tx_id = tx.transaction().get_id();

    state.rpc.submit_tx(tx).await.map_err(|e| {
        ApiServerWebServerError::ServerError(ApiServerWebServerServerError::RpcError(e.to_string()))
    })?;

    Ok(Json(
        json!({"tx_id": tx_id.to_hash().encode_hex::<String>()}),
    ))
}

pub async fn transactions<T: ApiServerStorage>(
    Query(params): Query<BTreeMap<String, String>>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    const OFFSET: &str = "offset";
    const ITEMS: &str = "items";
    const DEFAULT_NUM_ITEMS: u32 = 10;
    const MAX_NUM_ITEMS: u32 = 100;

    let offset = params
        .get(OFFSET)
        .map(|offset| u32::from_str(offset))
        .transpose()
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidOffset)
        })?
        .unwrap_or_default();

    let items = params
        .get(ITEMS)
        .map(|items| u32::from_str(items))
        .transpose()
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidNumItems)
        })?
        .unwrap_or(DEFAULT_NUM_ITEMS);
    ensure!(
        items <= MAX_NUM_ITEMS,
        ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidNumItems)
    );

    let txs = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_transactions_with_block(items, offset)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    let tip_height = best_block(&state).await?.block_height();
    let txs = txs
        .into_iter()
        .map(|(block, tx)| to_tx_json_with_block_info(&tx, &state.chain_config, tip_height, block))
        .collect();

    Ok(Json(serde_json::Value::Array(txs)))
}

pub async fn transaction<T: ApiServerStorage>(
    Path(transaction_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let (block, TransactionInfo { tx, additinal_info }) =
        get_transaction(&transaction_id, &state).await?;

    let confirmations = if let Some(block) = &block {
        let tip_height = best_block(&state).await?.block_height();
        tip_height.sub(block.block_height())
    } else {
        None
    };
    let mut json = tx_to_json(tx.transaction(), &additinal_info, &state.chain_config);
    let obj = json.as_object_mut().expect("object");

    obj.insert(
        "block_id".into(),
        block
            .as_ref()
            .map_or("".to_string(), |b| {
                b.block_id().to_hash().encode_hex::<String>()
            })
            .into(),
    );
    obj.insert(
        "timestamp".into(),
        block
            .as_ref()
            .map_or("".to_string(), |b| b.block_timestamp().to_string())
            .into(),
    );
    obj.insert(
        "confirmations".into(),
        confirmations.map_or("".to_string(), |c| c.to_string()).into(),
    );

    Ok(Json(json))
}

pub async fn transaction_merkle_path<T: ApiServerStorage>(
    Path(transaction_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let (block, transaction) = match get_transaction(&transaction_id, &state).await? {
        (Some(block_data), tx_info) => {
            let block = get_block(
                &block_data.block_id().to_hash().encode_hex::<String>(),
                &state,
            )
            .await?
            .block;
            (block, tx_info.tx.transaction().clone())
        }
        (None, _) => {
            return Err(ApiServerWebServerError::NotFound(
                ApiServerWebServerNotFoundError::TransactionNotPartOfBlock,
            ))
        }
    };

    let transaction_index: u32 = block
        .block
        .transactions()
        .iter()
        .position(|t| t.transaction().get_id() == transaction.get_id())
        .ok_or(ApiServerWebServerError::ServerError(
            ApiServerWebServerServerError::CannotFindTransactionInBlock,
        ))?
        .try_into()
        .map_err(|_| {
            ApiServerWebServerError::ServerError(
                ApiServerWebServerServerError::TransactionIndexOverflow,
            )
        })?;

    let merkle_tree = block
        .block
        .body()
        .merkle_tree_proxy()
        .map_err(|_| {
            ApiServerWebServerError::ServerError(
                ApiServerWebServerServerError::ErrorCalculatingMerkleTree,
            )
        })?
        .merkle_tree()
        .transaction_inclusion_proof(transaction_index)
        .map_err(|_| {
            ApiServerWebServerError::ServerError(
                ApiServerWebServerServerError::ErrorCalculatingMerklePath,
            )
        })?
        .into_hashes()
        .into_iter()
        .map(|h| h.encode_hex::<String>())
        .collect::<Vec<_>>();

    Ok(Json(json!({
    "block_id": block.block.get_id(),
    "transaction_index": transaction_index,
    "merkle_root": block.block.merkle_root().encode_hex::<String>(),
    "merkle_path": merkle_tree,
    })))
}

//
// address/
//

pub async fn address<T: ApiServerStorage>(
    Path(address): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let address =
        Address::<Destination>::from_str(&state.chain_config, &address).map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidAddress)
        })?;
    let tx = state.db.transaction_ro().await.map_err(|e| {
        logging::log::error!("internal error: {e}");
        ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
    })?;

    let transaction_history =
        tx.get_address_transactions(&address.to_string()).await.map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    // if there is no transaction history then return not found
    ensure!(
        !transaction_history.is_empty(),
        ApiServerWebServerError::NotFound(ApiServerWebServerNotFoundError::AddressNotFound,)
    );

    let coin_balance = tx
        .get_address_balance(&address.to_string(), CoinOrTokenId::Coin)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .unwrap_or(Amount::ZERO);

    let locked_coin_balance = tx
        .get_address_locked_balance(&address.to_string(), CoinOrTokenId::Coin)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .unwrap_or(Amount::ZERO);

    Ok(Json(json!({
    "coin_balance": amount_to_json(coin_balance, state.chain_config.coin_decimals()),
    "locked_coin_balance": amount_to_json(locked_coin_balance, state.chain_config.coin_decimals()),
    "transaction_history": transaction_history
    //TODO "token_balances": destination_summary.token_balances(),
    })))
}

pub async fn address_utxos<T: ApiServerStorage>(
    Path(address): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let address =
        Address::<Destination>::from_str(&state.chain_config, &address).map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidAddress)
        })?;

    let utxos = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_address_available_utxos(&address.to_string())
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    Ok(Json(
        utxos
            .into_iter()
            .map(|utxo| {
                json!({
                "outpoint": utxo_outpoint_to_json(&utxo.0),
                "utxo": txoutput_to_json(&utxo.1.output, &state.chain_config, &TokenDecimals::Single(utxo.1.token_decimals))})
            })
            .collect::<Vec<_>>(),
    ))
}

pub async fn all_address_utxos<T: ApiServerStorage>(
    Path(address): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let address =
        Address::<Destination>::from_str(&state.chain_config, &address).map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidAddress)
        })?;

    let utxos = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_address_all_utxos(&address.to_string())
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    Ok(Json(
        utxos
            .into_iter()
            .map(|utxo| {
                json!({
                "outpoint": utxo_outpoint_to_json(&utxo.0),
                "utxo": txoutput_to_json(&utxo.1.output, &state.chain_config, &TokenDecimals::Single(utxo.1.token_decimals))})
            })
            .collect::<Vec<_>>(),
    ))
}

pub async fn address_delegations<T: ApiServerStorage>(
    Path(address): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let address =
        Address::<Destination>::from_str(&state.chain_config, &address).map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidAddress)
        })?;

    let delegations = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_delegations_from_address(&address.into_object())
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    Ok(Json(
        delegations.into_iter().map(|(delegation_id, delegation)|
            json!({
            "delegation_id": Address::new(&state.chain_config, delegation_id).expect(
                "no error in encoding"
            ).as_str(),
            "pool_id": Address::new(&state.chain_config, *delegation.pool_id()).expect(
                "no error in encoding"
            ).as_str(),
            "next_nonce": delegation.next_nonce(),
            "spend_destination": Address::new(&state.chain_config, delegation.spend_destination().clone()).expect(
                "no error in encoding"
            ).as_str(),
            "balance": amount_to_json(*delegation.balance(), state.chain_config.coin_decimals()),
        })
        ).collect::<Vec<_>>(),
    ))
}

//
// pool/
//

enum PoolSorting {
    ByHeight,
    ByPledge,
}

impl FromStr for PoolSorting {
    type Err = ApiServerWebServerClientError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "by_height" => Ok(Self::ByHeight),
            "by_pledge" => Ok(Self::ByPledge),
            _ => Err(ApiServerWebServerClientError::InvalidPoolsSortOrder),
        }
    }
}

pub async fn pools<T: ApiServerStorage>(
    Query(params): Query<BTreeMap<String, String>>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    const OFFSET: &str = "offset";
    const ITEMS: &str = "items";
    const DEFAULT_NUM_ITEMS: u32 = 10;
    const MAX_NUM_ITEMS: u32 = 100;
    const SORT: &str = "sort";

    let offset = params
        .get(OFFSET)
        .map(|offset| u32::from_str(offset))
        .transpose()
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidOffset)
        })?
        .unwrap_or_default();

    let items = params
        .get(ITEMS)
        .map(|items| u32::from_str(items))
        .transpose()
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidNumItems)
        })?
        .unwrap_or(DEFAULT_NUM_ITEMS);
    ensure!(
        items <= MAX_NUM_ITEMS,
        ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidNumItems)
    );

    let sort = params
        .get(SORT)
        .map(|offset| PoolSorting::from_str(offset))
        .transpose()?
        .unwrap_or(PoolSorting::ByHeight);

    let db_tx = state.db.transaction_ro().await.map_err(|e| {
        logging::log::error!("internal error: {e}");
        ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
    })?;

    let pools = match sort {
        PoolSorting::ByHeight => db_tx.get_latest_pool_data(items, offset).await.map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?,
        PoolSorting::ByPledge => db_tx
            .get_pool_data_with_largest_staker_balance(items, offset)
            .await
            .map_err(|e| {
                logging::log::error!("internal error: {e}");
                ApiServerWebServerError::ServerError(
                    ApiServerWebServerServerError::InternalServerError,
                )
            })?,
    };

    let pools = pools.into_iter().map(|(pool_id, pool_data)| {
        let decommission_destination =
            Address::new(&state.chain_config, pool_data.decommission_destination().clone())
                .expect("no error in encoding");
        let pool_id = Address::new(&state.chain_config, pool_id).expect("no error in encoding");
        let vrf_key = Address::new(&state.chain_config, pool_data.vrf_public_key().clone())
            .expect("no error in encoding");
        json!({
            "pool_id": pool_id.as_str(),
            "decommission_destination": decommission_destination.as_str(),
            "staker_balance": amount_to_json(pool_data.staker_balance().expect("no overflow"), state.chain_config.coin_decimals()),
            "margin_ratio_per_thousand": pool_data.margin_ratio_per_thousand(),
            "cost_per_block": amount_to_json(pool_data.cost_per_block(), state.chain_config.coin_decimals()),
            "vrf_public_key": vrf_key.as_str(),
        })
    });

    Ok(Json(pools.collect::<Vec<_>>()))
}

pub async fn pool<T: ApiServerStorage>(
    Path(pool_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let pool_id = Address::from_str(&state.chain_config, &pool_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidPoolId)
        })?
        .into_object();

    let pool_data = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_pool_data(pool_id)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .ok_or(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::PoolNotFound,
        ))?;

    let decommission_destination = Address::new(
        &state.chain_config,
        pool_data.decommission_destination().clone(),
    )
    .expect("no error in encoding");
    let vrf_key = Address::new(&state.chain_config, pool_data.vrf_public_key().clone())
        .expect("no error in encoding");
    Ok(Json(json!({
        "decommission_destination": decommission_destination.as_str(),
        "staker_balance": amount_to_json(pool_data.staker_balance().expect("no overflow"), state.chain_config.coin_decimals()),
        "margin_ratio_per_thousand": pool_data.margin_ratio_per_thousand(),
        "cost_per_block": amount_to_json(pool_data.cost_per_block(), state.chain_config.coin_decimals()),
        "vrf_public_key": vrf_key.as_str(),
    })))
}

#[derive(Debug, Deserialize)]
pub struct TimeFilter {
    from: u64,
    to: u64,
}

pub async fn pool_block_stats<T: ApiServerStorage>(
    Path(pool_id): Path<String>,
    Query(params): Query<TimeFilter>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let pool_id = Address::from_str(&state.chain_config, &pool_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidPoolId)
        })?
        .into_object();

    let tx = state.db.transaction_ro().await.map_err(|e| {
        logging::log::error!("internal error: {e}");
        ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
    })?;

    let block_range = tx
        .get_block_range_from_time_range((
            BlockTimestamp::from_int_seconds(params.from),
            BlockTimestamp::from_int_seconds(params.to),
        ))
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    let pool_block_stats = tx
        .get_pool_block_stats(pool_id, block_range)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .ok_or(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::PoolNotFound,
        ))?;

    Ok(Json(json!({
        "block_count": pool_block_stats.block_count,
    })))
}

pub async fn pool_delegations<T: ApiServerStorage>(
    Path(pool_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let pool_id = Address::from_str(&state.chain_config, &pool_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidPoolId)
        })?
        .into_object();

    let delegations = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_pool_delegations(pool_id)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?;

    Ok(Json(
        delegations.into_iter().map(|(delegation_id, delegation)|
            json!({
            "delegation_id": Address::new(&state.chain_config, delegation_id).expect(
                "no error in encoding"
            ).as_str(),
            "next_nonce": delegation.next_nonce(),
            "spend_destination": Address::new(&state.chain_config, delegation.spend_destination().clone()).expect(
                "no error in encoding"
            ).as_str(),
            "balance": amount_to_json(*delegation.balance(), state.chain_config.coin_decimals()),
        })
        ).collect::<Vec<_>>(),
    ))
}

pub async fn delegation<T: ApiServerStorage>(
    Path(delegation_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let delegation_id = Address::from_string(&state.chain_config, delegation_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidPoolId)
        })?
        .into_object();

    let delegation = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_delegation(delegation_id)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .ok_or(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::DelegationNotFound,
        ))?;

    Ok(Json(json!({
        "spend_destination": Address::new(&state.chain_config, delegation.spend_destination().clone()).expect(
            "no error in encoding"
        ).as_str(),
        "balance": amount_to_json(*delegation.balance(), state.chain_config.coin_decimals()),
        "next_nonce": delegation.next_nonce(),
        "pool_id": Address::new(&state.chain_config, *delegation.pool_id()).expect(
            "no error in encoding"
        ).as_str(),
    })))
}

pub async fn token<T: ApiServerStorage>(
    Path(token_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let token_id = Address::from_str(&state.chain_config, &token_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidTokenId)
        })?
        .into_object();

    let token = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_fungible_token_issuance(token_id)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .ok_or(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::TokenNotFound,
        ))?;

    let (frozen, freezable, unfreezable) = match token.frozen {
        IsTokenFrozen::No(changable) => {
            let freezable = match changable {
                IsTokenFreezable::Yes => true,
                IsTokenFreezable::No => false,
            };
            (false, Some(freezable), None)
        }
        IsTokenFrozen::Yes(changable) => {
            let unfreezable = match changable {
                IsTokenUnfreezable::Yes => true,
                IsTokenUnfreezable::No => false,
            };
            (true, None, Some(unfreezable))
        }
    };

    Ok(Json(json!({
        "authority": Address::new(&state.chain_config, token.authority).expect(
            "no error in encoding"
        ).as_str(),
        "is_locked": token.is_locked,
        "circulating_supply": amount_to_json(token.circulating_supply, token.number_of_decimals),
        "token_ticker": to_json_string(&token.token_ticker),
        "metadata_uri": to_json_string(&token.metadata_uri),
        "number_of_decimals": token.number_of_decimals,
        "total_supply": token.total_supply,
        "frozen": frozen,
        "is_token_unfreezable": unfreezable,
        "is_token_freezable": freezable,
    })))
}

pub async fn nft<T: ApiServerStorage>(
    Path(nft_id): Path<String>,
    State(state): State<ApiServerWebServerState<Arc<T>, Arc<impl TxSubmitClient>>>,
) -> Result<impl IntoResponse, ApiServerWebServerError> {
    let nft_id = Address::from_str(&state.chain_config, &nft_id)
        .map_err(|_| {
            ApiServerWebServerError::ClientError(ApiServerWebServerClientError::InvalidNftId)
        })?
        .into_object();

    let nft = state
        .db
        .transaction_ro()
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .get_nft_token_issuance(nft_id)
        .await
        .map_err(|e| {
            logging::log::error!("internal error: {e}");
            ApiServerWebServerError::ServerError(ApiServerWebServerServerError::InternalServerError)
        })?
        .ok_or(ApiServerWebServerError::NotFound(
            ApiServerWebServerNotFoundError::NftNotFound,
        ))?;

    match nft {
        NftIssuance::V0(nft) => Ok(Json(json!({
            "authority": nft.metadata.creator
                .map(|creator| Address::new(&state.chain_config, Destination::PublicKey(creator.public_key))
                .expect("no error in encoding")
                .as_str().to_owned()
            ),
            "name": nft.metadata.name,
            "description": nft.metadata.description,
            "ticker": to_json_string(&nft.metadata.ticker),
            "icon_uri": nft.metadata.icon_uri.as_ref().as_ref().map(|b| to_json_string(b)),
            "additional_metadata_uri": nft.metadata.additional_metadata_uri.as_ref().as_ref().map(|b| to_json_string(b)),
            "media_uri": nft.metadata.media_uri.as_ref().as_ref().map(|b| to_json_string(b)),
            "media_hash": to_json_string(&nft.metadata.media_hash),
        }))),
    }
}
