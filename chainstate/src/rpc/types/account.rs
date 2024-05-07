// Copyright (c) 2024 RBB S.r.l
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

use common::{
    address::{AddressError, RpcAddress},
    chain::{
        tokens::{IsTokenUnfreezable, TokenId},
        AccountCommand, AccountSpending, ChainConfig, DelegationId, Destination, OrderId,
    },
    primitives::amount::RpcAmountOut,
};

use super::output::RpcOutputValue;

#[derive(Debug, Clone, serde::Serialize, rpc_description::HasValueHint)]
#[serde(tag = "type", content = "content")]
pub enum RpcAccountSpending {
    DelegationBalance {
        delegation_id: RpcAddress<DelegationId>,
        amount: RpcAmountOut,
    },
}

impl RpcAccountSpending {
    pub fn new(
        chain_config: &ChainConfig,
        spending: AccountSpending,
    ) -> Result<Self, AddressError> {
        let result = match spending {
            AccountSpending::DelegationBalance(id, amount) => {
                RpcAccountSpending::DelegationBalance {
                    delegation_id: RpcAddress::new(chain_config, id)?,
                    amount: RpcAmountOut::from_amount(amount, chain_config.coin_decimals()),
                }
            }
        };
        Ok(result)
    }
}

#[derive(Debug, Clone, serde::Serialize, rpc_description::HasValueHint)]
#[serde(tag = "type", content = "content")]
pub enum RpcAccountCommand {
    MintTokens {
        token_id: RpcAddress<TokenId>,
        amount: RpcAmountOut,
    },
    UnmintTokens {
        token_id: RpcAddress<TokenId>,
    },
    LockTokenSupply {
        token_id: RpcAddress<TokenId>,
    },
    FreezeToken {
        token_id: RpcAddress<TokenId>,
        is_unfreezable: bool,
    },
    UnfreezeToken {
        token_id: RpcAddress<TokenId>,
    },
    ChangeTokenAuthority {
        token_id: RpcAddress<TokenId>,
        new_authority: RpcAddress<Destination>,
    },
    CancelOrder {
        order_id: RpcAddress<OrderId>,
    },
    FillOrder {
        order_id: RpcAddress<OrderId>,
        fill_value: RpcOutputValue,
        destination: RpcAddress<Destination>,
    },
}

impl RpcAccountCommand {
    pub fn new(chain_config: &ChainConfig, command: &AccountCommand) -> Result<Self, AddressError> {
        let result = match command {
            AccountCommand::MintTokens(id, amount) => RpcAccountCommand::MintTokens {
                token_id: RpcAddress::new(chain_config, *id)?,
                amount: RpcAmountOut::from_amount(*amount, chain_config.coin_decimals()),
            },
            AccountCommand::UnmintTokens(id) => RpcAccountCommand::UnmintTokens {
                token_id: RpcAddress::new(chain_config, *id)?,
            },
            AccountCommand::LockTokenSupply(id) => RpcAccountCommand::LockTokenSupply {
                token_id: RpcAddress::new(chain_config, *id)?,
            },
            AccountCommand::FreezeToken(id, is_unfreezable) => RpcAccountCommand::FreezeToken {
                token_id: RpcAddress::new(chain_config, *id)?,
                is_unfreezable: match is_unfreezable {
                    IsTokenUnfreezable::No => false,
                    IsTokenUnfreezable::Yes => true,
                },
            },
            AccountCommand::UnfreezeToken(id) => RpcAccountCommand::UnfreezeToken {
                token_id: RpcAddress::new(chain_config, *id)?,
            },
            AccountCommand::ChangeTokenAuthority(id, destination) => {
                RpcAccountCommand::ChangeTokenAuthority {
                    token_id: RpcAddress::new(chain_config, *id)?,
                    new_authority: RpcAddress::new(chain_config, destination.clone())?,
                }
            }
            AccountCommand::CancelOrder(id) => RpcAccountCommand::CancelOrder {
                order_id: RpcAddress::new(chain_config, *id)?,
            },
            AccountCommand::FillOrder(id, fill, dest) => RpcAccountCommand::FillOrder {
                order_id: RpcAddress::new(chain_config, *id)?,
                fill_value: RpcOutputValue::new(chain_config, fill.clone())?,
                destination: RpcAddress::new(chain_config, dest.clone())?,
            },
        };
        Ok(result)
    }
}
