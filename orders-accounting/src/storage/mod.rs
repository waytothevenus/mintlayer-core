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

use common::{chain::OrderId, primitives::Amount};
use std::ops::{Deref, DerefMut};

use crate::data::OrderData;

pub mod db;
pub mod in_memory;

pub trait OrdersAccountingStorageRead {
    type Error: std::error::Error;

    fn get_order_data(&self, id: &OrderId) -> Result<Option<OrderData>, Self::Error>;
    fn get_ask_balance(&self, id: &OrderId) -> Result<Option<Amount>, Self::Error>;
    fn get_give_balance(&self, id: &OrderId) -> Result<Option<Amount>, Self::Error>;
}

pub trait OrdersAccountingStorageWrite: OrdersAccountingStorageRead {
    fn set_order_data(&mut self, id: &OrderId, data: &OrderData) -> Result<(), Self::Error>;
    fn del_order_data(&mut self, id: &OrderId) -> Result<(), Self::Error>;

    fn set_ask_balance(&mut self, id: &OrderId, balance: &Amount) -> Result<(), Self::Error>;
    fn del_ask_balance(&mut self, id: &OrderId) -> Result<(), Self::Error>;

    fn set_give_balance(&mut self, id: &OrderId, balance: &Amount) -> Result<(), Self::Error>;
    fn del_give_balance(&mut self, id: &OrderId) -> Result<(), Self::Error>;
}

impl<T> OrdersAccountingStorageRead for T
where
    T: Deref,
    <T as Deref>::Target: OrdersAccountingStorageRead,
{
    type Error = <T::Target as OrdersAccountingStorageRead>::Error;

    fn get_order_data(&self, id: &OrderId) -> Result<Option<OrderData>, Self::Error> {
        self.deref().get_order_data(id)
    }

    fn get_ask_balance(&self, id: &OrderId) -> Result<Option<Amount>, Self::Error> {
        self.deref().get_ask_balance(id)
    }

    fn get_give_balance(&self, id: &OrderId) -> Result<Option<Amount>, Self::Error> {
        self.deref().get_give_balance(id)
    }
}

impl<T> OrdersAccountingStorageWrite for T
where
    T: DerefMut,
    <T as Deref>::Target: OrdersAccountingStorageWrite,
{
    fn set_order_data(&mut self, id: &OrderId, data: &OrderData) -> Result<(), Self::Error> {
        self.deref_mut().set_order_data(id, data)
    }

    fn del_order_data(&mut self, id: &OrderId) -> Result<(), Self::Error> {
        self.deref_mut().del_order_data(id)
    }

    fn set_ask_balance(&mut self, id: &OrderId, balance: &Amount) -> Result<(), Self::Error> {
        self.deref_mut().set_ask_balance(id, balance)
    }

    fn del_ask_balance(&mut self, id: &OrderId) -> Result<(), Self::Error> {
        self.deref_mut().del_ask_balance(id)
    }

    fn set_give_balance(&mut self, id: &OrderId, balance: &Amount) -> Result<(), Self::Error> {
        self.deref_mut().set_give_balance(id, balance)
    }

    fn del_give_balance(&mut self, id: &OrderId) -> Result<(), Self::Error> {
        self.deref_mut().del_give_balance(id)
    }
}
