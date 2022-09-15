use std::collections::{btree_map::Entry, BTreeMap};

use common::{
    chain::{
        calculate_tx_index_from_block, signature::Transactable, OutPoint, OutPointSourceId,
        TxMainChainIndex,
    },
    primitives::Idable,
};

use crate::ConnectTransactionError;

use super::{cached_operation::CachedInputsOperation, BlockTransactableRef};

pub struct TxIndexCache {
    data: BTreeMap<OutPointSourceId, CachedInputsOperation>,
}

impl TxIndexCache {
    pub fn new() -> Self {
        Self {
            data: BTreeMap::new(),
        }
    }

    pub fn add_tx_index(
        &mut self,
        spend_ref: BlockTransactableRef,
    ) -> Result<(), ConnectTransactionError> {
        let tx_index = match spend_ref {
            BlockTransactableRef::Transaction(block, tx_num) => {
                CachedInputsOperation::Write(calculate_tx_index_from_block(block, tx_num)?)
            }
            BlockTransactableRef::BlockReward(block) => {
                match block.block_reward_transactable().outputs() {
                    Some(outputs) => CachedInputsOperation::Write(TxMainChainIndex::new(
                        block.get_id().into(),
                        outputs
                            .len()
                            .try_into()
                            .map_err(|_| ConnectTransactionError::InvalidOutputCount)?,
                    )?),
                    None => return Ok(()), // no outputs to add
                }
            }
        };

        let outpoint_source_id = Self::outpoint_source_id_from_spend_ref(spend_ref)?;

        match self.data.entry(outpoint_source_id) {
            Entry::Occupied(_) => {
                return Err(ConnectTransactionError::OutputAlreadyPresentInInputsCache)
            }
            Entry::Vacant(entry) => entry.insert(tx_index),
        };
        Ok(())
    }

    pub fn remove_tx_index(
        &mut self,
        spend_ref: BlockTransactableRef,
    ) -> Result<(), ConnectTransactionError> {
        let tx_index = CachedInputsOperation::Erase;
        let outpoint_source_id = Self::outpoint_source_id_from_spend_ref(spend_ref)?;

        self.data.insert(outpoint_source_id, tx_index);
        Ok(())
    }

    pub fn fetch_and_cache<F>(
        &mut self,
        outpoint: &OutPoint,
        fetcher_func: F,
    ) -> Result<(), ConnectTransactionError>
    where
        F: Fn(&OutPointSourceId) -> Result<Option<TxMainChainIndex>, ConnectTransactionError>,
    {
        match self.data.entry(outpoint.tx_id()) {
            Entry::Occupied(_) => (),
            Entry::Vacant(entry) => {
                // Maybe the utxo is in a previous block?
                let tx_index = fetcher_func(&outpoint.tx_id())?
                    .ok_or(ConnectTransactionError::MissingOutputOrSpent)?;
                entry.insert(CachedInputsOperation::Read(tx_index));
            }
        }
        Ok(())
    }

    fn outpoint_source_id_from_spend_ref(
        spend_ref: BlockTransactableRef,
    ) -> Result<OutPointSourceId, ConnectTransactionError> {
        let outpoint_source_id = match spend_ref {
            BlockTransactableRef::Transaction(block, tx_num) => {
                let tx = block.transactions().get(tx_num).ok_or_else(|| {
                    ConnectTransactionError::InvariantErrorTxNumWrongInBlock(tx_num, block.get_id())
                })?;
                let tx_id = tx.get_id();
                OutPointSourceId::from(tx_id)
            }
            BlockTransactableRef::BlockReward(block) => OutPointSourceId::from(block.get_id()),
        };
        Ok(outpoint_source_id)
    }

    pub fn get_from_cached_mut(
        &mut self,
        outpoint: &OutPointSourceId,
    ) -> Result<&mut CachedInputsOperation, ConnectTransactionError> {
        let result = match self.data.get_mut(outpoint) {
            Some(tx_index) => tx_index,
            None => return Err(ConnectTransactionError::PreviouslyCachedInputNotFound),
        };
        Ok(result)
    }

    // TODO(PR): rename this
    pub fn get_from_cached(
        &self,
        outpoint: &OutPointSourceId,
    ) -> Result<Option<&TxMainChainIndex>, ConnectTransactionError> {
        let result = match self.data.get(outpoint) {
            Some(tx_index) => tx_index.get_tx_index(),
            None => return Err(ConnectTransactionError::PreviouslyCachedInputNotFound),
        };
        Ok(result)
    }

    pub fn take(self) -> BTreeMap<OutPointSourceId, CachedInputsOperation> {
        self.data
    }
}

// TODO: write tests
