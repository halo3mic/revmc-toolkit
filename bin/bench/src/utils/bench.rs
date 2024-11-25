use std::time::{Instant, Duration};
use revmc_toolkit_build::{CompilerOptions, OptimizationLevelDeseralizable};
use tracing::info;


pub fn measure_execution_time<F, R>(mut f: F, warmup_ms: u32, measurement_ms: u32) -> f64
where
    F: FnMut() -> R,
{
    info!("Warming up for {warmup_ms} ms");
    let warm_up_duration = Duration::from_millis(warmup_ms as u64);
    let start = Instant::now();
    let mut warmup_iter = 0;
    loop {
        f();
        if Instant::now() - start > warm_up_duration {
            break;
        }
        warmup_iter += 1;
    }

    let measurement_iter = warmup_iter * measurement_ms / warmup_ms;
    info!("Measuring with {measurement_iter} iterations");
    let start = Instant::now();
    for _ in 0..measurement_iter {
        f();
    }
    let m_duration = (Instant::now() - start).as_nanos();

    m_duration as f64 / measurement_iter as f64
}

pub fn time_fn<F, R>(mut fnc: F) -> Result<(R, Duration)> 
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
    Ok((res, elapsed))
}

pub(crate) struct RunConfig<T, U> {
    pub aot_dir_path: PathBuf,
    pub reth_db_path: T,
    pub compile_selection: U,
    pub comp_opt_level: OptimizationLevelDeseralizable,
}


use tracing::warn;
use std::path::PathBuf;
use eyre::{OptionExt, Result};
use revm::primitives::B256;
use reth_provider::{BlockReader, ReceiptProvider, TransactionsProvider, ProviderFactory};
use reth_db::DatabaseEnv;
use revmc_toolkit_load::RevmcExtCtx;
use revmc_toolkit_sim::sim_builder::{Simulation, StateProviderCacheDB};

// todo: check pre-execution result + optimize for sequential txs
// Expect some native touches for cases where bytecode of the contract is changed during the block execution
pub fn check_tx_sim_validity(
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
            let _ratio = touch_counter.non_native as f32 / touch_counter.overall as f32;
            // todo: will always be wrong for gas guzzlers
            // warn!("invalid touch count for {account:?}: expected all non-native, found {:.2?}%", ratio*100.);
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
        // todo: will always be wrong for gas guzzlers
        let msg = format!("invalid touch count: expected all non-native, found {:.2?}%", ratio*100.);
        if non_native > 0 {
            warn!("{msg}");
        } else {
            return Err(eyre::eyre!("{msg}"));
        }
    }

    Ok(())
}

pub fn compile_opt_from_aot_path(aot_path: PathBuf) -> CompilerOptions {
    let mut opt = CompilerOptions::default();
    opt.out_dir = aot_path;
    opt
}