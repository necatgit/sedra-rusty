use crate::mempool::{errors::RuleResult, model::pool::Pool, Mempool};
use kaspa_consensus_core::{
    api::{
        args::{TransactionValidationArgs, TransactionValidationBatchArgs},
        ConsensusApi,
    },
    constants::UNACCEPTED_DAA_SCORE,
    tx::{MutableTransaction, UtxoEntry},
};
use kaspa_mining_errors::mempool::RuleError;

/// SEDRA (security back-port, upstream rusty-kaspa 778b21e2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PopulateError {
    ImpossibleOutpoint,
}

impl From<PopulateError> for RuleError {
    fn from(err: PopulateError) -> Self {
        match err {
            PopulateError::ImpossibleOutpoint => RuleError::RejectImpossibleOutpoint,
        }
    }
}

impl Mempool {
    pub(crate) fn populate_mempool_entries(&self, transaction: &mut MutableTransaction) -> Result<(), PopulateError> {
        for (i, input) in transaction.tx.inputs.iter().enumerate() {
            if let Some(parent) = self.transaction_pool.get(&input.previous_outpoint.transaction_id) {
                // SEDRA (security back-port, upstream 778b21e2): the index comes from the incoming transaction
                let output =
                    parent.mtx.tx.outputs.get(input.previous_outpoint.index as usize).ok_or(PopulateError::ImpossibleOutpoint)?;
                transaction.entries[i] =
                    Some(UtxoEntry::new(output.value, output.script_public_key.clone(), UNACCEPTED_DAA_SCORE, false));
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_mempool_transaction(
    consensus: &dyn ConsensusApi,
    transaction: &mut MutableTransaction,
    args: &TransactionValidationArgs,
) -> RuleResult<()> {
    Ok(consensus.validate_mempool_transaction(transaction, args)?)
}

pub(crate) fn validate_mempool_transactions_in_parallel(
    consensus: &dyn ConsensusApi,
    transactions: &mut [MutableTransaction],
    args: &TransactionValidationBatchArgs,
) -> Vec<RuleResult<()>> {
    consensus.validate_mempool_transactions_in_parallel(transactions, args).into_iter().map(|x| x.map_err(RuleError::from)).collect()
}

pub(crate) fn populate_mempool_transactions_in_parallel(
    consensus: &dyn ConsensusApi,
    transactions: &mut [MutableTransaction],
) -> Vec<RuleResult<()>> {
    consensus.populate_mempool_transactions_in_parallel(transactions).into_iter().map(|x| x.map_err(RuleError::from)).collect()
}
