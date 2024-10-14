
use std::path::PathBuf;
use revmc_toolkit_load::RevmcExtCtx;
use tracing::warn;
use eyre::{OptionExt, Result};
use revm::primitives::{B256, Bytes};
use reth_provider::{BlockReader, ReceiptProvider, TransactionsProvider};

use crate::utils::sim::{SimCall, SimConfig, SimRunType, self as sim_utils};
use revmc_toolkit_utils::evm::make_provider_factory;
use revmc_toolkit_sim::sim_builder::{BlockPart, Simulation, StateProviderCacheDB};
use reth_provider::ProviderFactory;
use reth_db::DatabaseEnv;
use std::time::{Instant, Duration};
use super::benches::BenchConfig;


pub fn run_call(
    call: SimCall, 
    call_input: Bytes, 
    run_type: SimRunType, 
    aot_dir: Option<&PathBuf>
) -> Result<()> {
    println!("CallType: {call:?} with input: {call_input:?}");

    let bytecode = call.bytecode().original_bytes().into();
    println!("Bytecode: {}", hex::encode(&bytecode));
    let ext_ctx = sim_utils::make_ext_ctx(
        run_type, 
        vec![bytecode], 
        aot_dir,
    )?;
    let mut sim = SimConfig::from(ext_ctx)
        .make_call_sim(call, call_input.clone())?;
    let (_result, elapsed) = time_fn(|| sim.run())?;

    println!("CallType: {call:?} ran successfully");
    println!("Elapsed: {:?}", elapsed);

    Ok(())
}

pub fn run_tx(tx_hash: B256, run_type: SimRunType, config: &BenchConfig) -> Result<()> {
    println!("TxHash: {tx_hash:?}");

    let provider_factory = make_provider_factory(&config.reth_db_path)?;
    let (ext_ctx, is_native_exe) = 
        match &run_type {
            SimRunType::AOTCompiled | SimRunType::JITCompiled => {
                let bytecode = config.compile_selection.bytecodes(provider_factory.clone(), Some(vec![tx_hash]))?;
                let ctx = sim_utils::make_ext_ctx(run_type, bytecode, Some(&config.dir_path))?
                    .with_touch_tracking();
                (ctx, false)
            }
            SimRunType::Native => {
                (RevmcExtCtx::default().with_touch_tracking(), true)
            }
        };

    let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx).make_tx_sim(tx_hash)?;
    let (_result, elapsed) = time_fn(|| sim.run())?;
    
    check_tx_sim_validity(
        &provider_factory, 
        &mut sim, 
        vec![tx_hash], 
        is_native_exe, 
    )?;

    println!("TxHash: {tx_hash:?} ran successfully");
    println!("Elapsed: {:?}", elapsed);

    Ok(())
}

pub fn run_block(
    block_num: u64, 
    run_type: SimRunType, 
    config: &BenchConfig, 
    block_chunk: Option<BlockPart>
) -> Result<()> {
    println!("BlockNum: {block_num:?}");

    let provider_factory = make_provider_factory(&config.reth_db_path)?;
    let mut block_txs = provider_factory.block(block_num.into())?
        .ok_or_eyre("block not found")?
        .body
        .iter()
        .map(|tx| tx.hash)
        .collect::<Vec<_>>();
    if let Some(block_chunk) = block_chunk {
        block_txs = block_chunk.split_txs(block_txs).0;
    }

    let is_native_exe = matches!(run_type, SimRunType::Native);
    let ext_ctx = 
        match run_type {
            SimRunType::AOTCompiled | SimRunType::JITCompiled => {
                let bytecode = config.compile_selection.bytecodes(provider_factory.clone(), Some(block_txs.clone()))?;
                sim_utils::make_ext_ctx(run_type, bytecode, Some(&config.dir_path))?
                    .with_touch_tracking()
            }
            SimRunType::Native => RevmcExtCtx::default()
                .with_touch_tracking()
        };

    let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx)
        .make_block_sim(block_num, block_chunk)?;
    let (_result, elapsed) = time_fn(|| sim.run())?;
    
    check_tx_sim_validity(
        &provider_factory, 
        &mut sim, 
        block_txs, 
        is_native_exe, 
    )?;

    println!("Block: {block_num:?} ran successfully");
    println!("Elapsed: {:?}", elapsed);

    Ok(())
}

// todo: check pre-execution result + optimize for sequential txs
// Expect some native touches for cases where bytecode of the contract is changed during the block execution
fn check_tx_sim_validity(
    provider_factory: &ProviderFactory<DatabaseEnv>,
    sim: &mut Simulation<RevmcExtCtx, StateProviderCacheDB>, 
    tx_hashes: Vec<B256>,
    native_exe: bool,
) -> Result<()> {
    let sim_results = sim.run()?;

    for (i, tx_hash) in tx_hashes.into_iter().enumerate() {        
        let (_tx, meta) = provider_factory.transaction_by_hash_with_meta(tx_hash)?
            .ok_or_eyre("tx not found")?;
        let receipt = provider_factory.receipt_by_hash(tx_hash)?
            .ok_or_eyre("receipt not found")?;
        let block = provider_factory.block_by_number(meta.block_number)?
            .ok_or_eyre("block not found")?;
        let prev_tx_cumm_gas = 
            if meta.index > 0 {
                let prev_tx_hash = &block.body[meta.index as usize - 1].hash;
                provider_factory.receipt_by_hash(*prev_tx_hash)?
                    .ok_or_eyre("prev-tx-receipt not found")?
                    .cumulative_gas_used
            } else {
                0
            };
        let expected_gas_used = receipt.cumulative_gas_used - prev_tx_cumm_gas;

        let res = &sim_results[i];
        if expected_gas_used != res.gas_used {
            return Err(eyre::eyre!("gas-used mismatch for {tx_hash}: expected {expected_gas_used} found {}", res.gas_used));
        }
        if receipt.success != res.success {
            return Err(eyre::eyre!("success mismatch for {tx_hash}: expected {} found {}", receipt.success, res.success));
        }
    }

    let touches = sim.evm().context.external.touches.as_ref()
        .ok_or_eyre("touches not found")?;

    let mut overall = 0;
    let mut non_native = 0;
    for (account, touch_counter) in touches.inner().iter() {
        overall += touch_counter.overall;
        non_native += touch_counter.non_native;

        if touch_counter.overall == 0 {
            warn!("invalid touch count for {account:?}: expected >0");
        }
        if native_exe && (touch_counter.non_native > 0) {
            let ratio = 1. - touch_counter.non_native as f32 / touch_counter.overall as f32;
            warn!("invalid touch count for {account:?}: expected all native, found {:.2?}%", ratio*100.);
        }
        if !native_exe && (touch_counter.non_native != touch_counter.overall) {
            let ratio = touch_counter.non_native as f32 / touch_counter.overall as f32;
            warn!("invalid touch count for {account:?}: expected all non-native, found {:.2?}%", ratio*100.);
        }
    }

    if native_exe && (non_native != 0) {
        let ratio = 1. - non_native as f32 / overall as f32;
        let msg = format!("invalid touch count: expected all native, found {:.2?}%", ratio*100.);
        if non_native < overall {
            warn!("{msg}");
        } else {
            return Err(eyre::eyre!("{msg}"));
        }
    }
    if !native_exe && (non_native != overall) {
        let ratio = non_native as f32 / overall as f32;
        let msg = format!("invalid touch count: expected all non-native, found {:.2?}%", ratio*100.);
        if non_native > 0 {
            warn!("{msg}");
        } else {
            return Err(eyre::eyre!("{msg}"));
        }
    }

    Ok(())
}

fn time_fn<F, R>(mut fnc: F) -> Result<(R, Duration)> 
where
    F: FnMut() -> Result<R>,
{
    // Warmup
    for _ in 0..5 {
        fnc()?;
    }
    let start = Instant::now();
    let res = fnc()?;
    let elapsed = start.elapsed();
    println!("Elapsed: {:?}", elapsed);
    Ok((res, elapsed))
}