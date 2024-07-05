use reth_provider::StateProvider;
use reth_revm::{database::StateProviderDatabase, DatabaseRef};
use reth_primitives::{PooledTransactionsElement, Address, transaction::FillTxEnv};
use reth_rpc_types::{EthCallBundleResponse, EthCallBundleTransactionResult};
use revm::{
    db::CacheDB, primitives::{
        BlockEnv, CfgEnvWithHandlerCfg, EnvKzgSettings, EnvWithHandlerCfg, FixedBytes, ResultAndState, TxEnv, U256,
    }, DatabaseCommit,
};

use eyre::{OptionExt, Result};


// modified code from reth's EthBundle::call_bundle
pub fn sim_txs(
    transactions: Vec<(PooledTransactionsElement, Address)>,
    cfg: CfgEnvWithHandlerCfg, 
    block_env: BlockEnv, 
    state: Box<dyn StateProvider>
) -> Result<EthCallBundleResponse> {
    let coinbase = block_env.coinbase;
    let basefee = Some(block_env.basefee.to::<u64>());
    let env = EnvWithHandlerCfg::new_with_cfg_env(cfg, block_env, TxEnv::default());
    let db = CacheDB::new(StateProviderDatabase::new(state));

    let initial_coinbase = DatabaseRef::basic_ref(&db, coinbase)?
        .map(|acc| acc.balance)
        .unwrap_or_default();
    let mut coinbase_balance_before_tx = initial_coinbase;
    let mut coinbase_balance_after_tx = initial_coinbase;
    let mut total_gas_used = 0u64;
    let mut total_gas_fess = U256::ZERO;
    let mut hash_bytes = Vec::with_capacity(32 * transactions.len());

    let mut evm =
        revm::Evm::builder().with_db(db).with_env_with_handler_cfg(env).build();

    let mut results = Vec::with_capacity(transactions.len());
    let mut transactions = transactions.into_iter().peekable();

    while let Some((tx, signer)) = transactions.next() {
        // Verify that the given blob data, commitments, and proofs are all valid for
        // this transaction.
        if let PooledTransactionsElement::BlobTransaction(ref tx) = tx {
            tx.validate(EnvKzgSettings::Default.get())?
        }

        let tx = tx.into_ecrecovered_transaction(signer);

        hash_bytes.extend_from_slice(tx.hash().as_slice());
        let gas_price = tx
            .effective_tip_per_gas(basefee)
            .ok_or_eyre("RpcInvalidTransactionError::FeeCapTooLow")?;
        tx.fill_tx_env(evm.tx_mut(), signer);
        let ResultAndState { result, state } = evm.transact()?;

        let gas_used = result.gas_used();
        total_gas_used += gas_used;

        let gas_fees = U256::from(gas_used) * U256::from(gas_price);
        total_gas_fess += gas_fees;

        // coinbase is always present in the result state
        coinbase_balance_after_tx =
            state.get(&coinbase).map(|acc| acc.info.balance).unwrap_or_default();
        let coinbase_diff =
            coinbase_balance_after_tx.saturating_sub(coinbase_balance_before_tx);
        let eth_sent_to_coinbase = coinbase_diff.saturating_sub(gas_fees);

        // update the coinbase balance
        coinbase_balance_before_tx = coinbase_balance_after_tx;

        // set the return data for the response
        let (value, revert) = if result.is_success() {
            let value = result.into_output().unwrap_or_default();
            (Some(value), None)
        } else {
            let revert = result.into_output().unwrap_or_default();
            (None, Some(revert))
        };

        let tx_res = EthCallBundleTransactionResult {
            coinbase_diff,
            eth_sent_to_coinbase,
            from_address: tx.signer(),
            gas_fees,
            gas_price: U256::from(gas_price),
            gas_used,
            to_address: tx.to(),
            tx_hash: tx.hash(),
            value,
            revert,
        };
        results.push(tx_res);

        // need to apply the state changes of this call before executing the
        // next call
        if transactions.peek().is_some() {
            // need to apply the state changes of this call before executing
            // the next call
            evm.context.evm.db.commit(state)
        }
    }

    // populate the response

    let coinbase_diff = coinbase_balance_after_tx.saturating_sub(initial_coinbase);
    let eth_sent_to_coinbase = coinbase_diff.saturating_sub(total_gas_fess);
    let bundle_gas_price =
        coinbase_diff.checked_div(U256::from(total_gas_used)).unwrap_or_default();
    let res = EthCallBundleResponse {
        bundle_gas_price,
        bundle_hash: FixedBytes::default(),
        coinbase_diff,
        eth_sent_to_coinbase,
        gas_fees: total_gas_fess,
        results,
        state_block_number: 0,
        total_gas_used,
    };

    Ok(res)
}
