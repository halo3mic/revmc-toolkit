use std::{path::PathBuf, time::Duration};
use tracing::{info, warn, span, Level};
use criterion::Criterion;
use eyre::{OptionExt, Result};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};

use revm::primitives::{B256, Bytes};
use reth_provider::{ProviderFactory, BlockReader};
use reth_db::DatabaseEnv;

use revmc_toolkit_load::{EvmCompilerFns, RevmcExtCtx};
use revmc_toolkit_utils::{evm as evm_utils, rnd as rnd_utils};
use revmc_toolkit_sim::{sim_builder::BlockPart, bytecode_touches};
use crate::cli::BytecodeSelectionCli;
use crate::utils::{
    sim::{SimCall, SimConfig, SimRunType, BytecodeSelection, self as sim_utils},
    bench::{self as bench_utils, RunConfig},
};


// todo: sample_size and measurement_time as args
// todo: add jit optionally


impl RunConfig<PathBuf, BytecodeSelection> {
    pub fn new(
        dir_path: PathBuf, 
        reth_db_path: PathBuf, 
        compile_selection: BytecodeSelection
    ) -> Self {
        Self { dir_path, reth_db_path, compile_selection }
    }

    pub fn bench_tx(&self, tx_hash: B256) -> Result<()> {
        let span = span!(Level::INFO, "bench_tx");
        let _guard = span.enter();
        info!("TxHash: {:?}", tx_hash);
        let mut criterion = Criterion::default()
            .sample_size(100)
            .measurement_time(Duration::from_secs(5));
    
        let provider_factory = evm_utils::make_provider_factory(&self.reth_db_path)?;    
        let bytecodes = self.compile_selection.bytecodes(provider_factory.clone(), Some(vec![tx_hash]))?;
    
        for (symbol, run_type) in [
            ("aot", SimRunType::AOTCompiled),
            ("native", SimRunType::Native),
            // ("jit", SimRunType::JITCompiled),
        ] {
            info!("Running {}", symbol.to_uppercase());
    
            let ext_ctx = sim_utils::make_ext_ctx(run_type.clone(), bytecodes.clone(), Some(&self.dir_path))?
                .with_touch_tracking();
            let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx)
                .make_tx_sim(tx_hash)?;
    
            bench_utils::check_tx_sim_validity(
                &provider_factory,
                &mut sim,
                vec![tx_hash],
                matches!(run_type, SimRunType::Native),
            )?;

            criterion.bench_function(&format!("sim_tx_{symbol}"), |b| {
                b.iter(|| { sim.run() })
            });
        }
        Ok(())
    }
    
    pub fn bench_block(&self, block_num: u64, block_chunk: Option<BlockPart>) -> Result<()> {
        let span = span!(Level::INFO, "bench_block");
        let _guard = span.enter();
        info!("Block: {:?}", block_num);
        let mut criterion = Criterion::default()
            .sample_size(100)
            .measurement_time(Duration::from_secs(5));
    
        let provider_factory = evm_utils::make_provider_factory(&self.reth_db_path)?;
        let bytecodes = self.compile_selection.bytecodes(
            provider_factory.clone(),
            Some(txs_for_block(&provider_factory, block_num)?)
        )?;
    
        for (symbol, run_type) in [
            ("native", SimRunType::Native),
            // ("jit", SimRunType::JITCompiled),
            ("aot", SimRunType::AOTCompiled),
        ] {
            info!("Running {}", symbol.to_uppercase());
    
            let ext_ctx = sim_utils::make_ext_ctx(run_type.clone(), bytecodes.clone(), Some(&self.dir_path))?
                .with_touch_tracking();
            let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx)
                .make_block_sim(block_num, block_chunk)?;
    
            bench_utils::check_tx_sim_validity(
                &provider_factory,
                &mut sim,
                txs_for_block(&provider_factory, block_num)?,
                matches!(run_type, SimRunType::Native),
            )?;

            criterion.bench_function(&format!("sim_block_{symbol}"), |b| {
                b.iter(|| { sim.run() })
            });
        }
        Ok(())
    }

    // todo: improve
    pub fn bench_block_range(&self, args: BlockRangeArgs) -> Result<()> {
        let span = span!(Level::INFO, "bench_block_range");
        let _guard = span.enter();

        let provider_factory = evm_utils::make_provider_factory(&self.reth_db_path)?;
        let mut writer = csv::WriterBuilder::new().from_path(&args.out_path)?;

        let bytecodes = 
            if let BytecodeSelection::GasGuzzlers { config: gconfig, size_limit } = &self.compile_selection {
                Some(gconfig
                    .find_gas_guzzlers(provider_factory.clone())?
                    .into_top_guzzlers(Some(*size_limit)))
            } else {
                None
            };
        let mut compiled_fns_inner = EvmCompilerFns::default();

        let block_iter = args.block_iter;
        for (symbol, run_type) in [
            ("native", SimRunType::Native),
            ("aot", SimRunType::AOTCompiled),
            // ("jit", SimRunType::JITCompiled),
        ] {
            if let Some(bytecodes) = bytecodes.as_ref() {
                compiled_fns_inner = sim_utils::make_compiled_fns(run_type.clone(), bytecodes.clone(), Some(&self.dir_path))?;
            }
            let measurements = block_iter.clone().into_par_iter().map(|block_num| {
                info!("Running {} for block {block_num}", symbol.to_uppercase());

                let _compiled_fns = 
                    if bytecodes.is_none() {
                        let txs = txs_for_block(&provider_factory, block_num)?;
                        let bytecodes = bytecode_touches::find_touched_bytecode(provider_factory.clone(), txs)?
                            .into_iter().collect();
                        sim_utils::make_compiled_fns(run_type.clone(), bytecodes, Some(&self.dir_path))?
                    } else {
                        compiled_fns_inner.clone()
                    };

                let sim_config = SimConfig::new(
                    provider_factory.clone(),
                    RevmcExtCtx::from(_compiled_fns).with_touch_tracking(),
                );

                let (mut sim, m_id) = 
                    if args.run_rnd_txs {
                        let block = provider_factory.block(block_num.into())?
                            .ok_or_eyre("Block not found")?;
                        if block.body.is_empty() {
                            warn!("Found empty block {}, skipping", block_num);
                            return Ok::<Option<MeasureRecord>, eyre::ErrReport>(None);
                        }
                        let txs_len = args.block_chunk
                            .map(|chunk| {
                                match chunk {
                                    BlockPart::TOB(c) => (block.body.len() as f32 * c) as usize,
                                    BlockPart::BOB(c) => (block.body.len() as f32 * (1. - c)) as usize,
                                }
                            })
                            .unwrap_or(block.body.len());
                        let tx_index = rnd_utils::random_sequence(0, txs_len, 1, args.seed)?[0]; // todo: seed in params
                        let tx_hash = block.body[tx_index].hash;
                        info!("Running random tx: {tx_hash:?}");

                        (sim_config.make_tx_sim(tx_hash)?, MeasureId::Tx(tx_hash))
                    } else {
                        (sim_config.make_block_sim(block_num, args.block_chunk)?, MeasureId::Block(block_num))
                    };
                
                info!("Checking validity of txs for block {block_num}");
                let check_res = bench_utils::check_tx_sim_validity(
                    &provider_factory,
                    &mut sim,
                    txs_for_block(&provider_factory, block_num)?,
                    matches!(run_type, SimRunType::Native),
                );
                if let Err(e) = &check_res {
                    warn!("Check failed for block {block_num} with: {e}");
                }
                
                let exe_time = bench_utils::measure_execution_time(
                    || { sim.run() }, 
                    args.warmup_ms, 
                    args.measurement_ms
                );
                Ok(Some(MeasureRecord {
                    run_type: symbol.to_string(),
                    id: m_id,
                    exe_time,
                    err: check_res.err().map(|e| e.to_string()),
                }))
            }).collect::<Vec<Result<_>>>();

            for m in measurements {
                match m {
                    Err(e) => warn!("Error: {e}"),
                    Ok(None) => continue,
                    Ok(Some(record)) => writer.serialize(record)?,
                }
            }
            writer.flush()?;
        }

        info!("Finished comparing block range ✨");
        info!("The records are written to {}", args.out_path.display());
        Ok(())
    }

    
}

impl<T, U> RunConfig<T, U> {

    pub fn bench_call(&self, call: SimCall, call_input: Bytes) -> Result<()> {
        let span = span!(Level::INFO, "bench_call");
        let _guard = span.enter();
        info!("Call: {:?}", call);
        let mut criterion = Criterion::default()
            .sample_size(100)
            .measurement_time(Duration::from_secs(5));
        let mut group = criterion.benchmark_group("call_benchmarks");
        
        for (symbol, run_type) in [
            ("jit", SimRunType::JITCompiled),
            ("aot", SimRunType::AOTCompiled),
            ("native", SimRunType::Native),
        ] {
            info!("Running {}", symbol.to_uppercase());
    
            let bytecode = call.bytecode().original_bytes().into();
            let ext_ctx = sim_utils::make_ext_ctx(
                run_type, 
                vec![bytecode], 
                Some(&self.dir_path)
            )?;
            let mut sim = SimConfig::from(ext_ctx)
                .make_call_sim(call, call_input.clone())?;
    
            // check_fn_validity(&mut fnc)?;
            group.bench_function(&format!("sim_call_{symbol}"), |b| {
                b.iter(|| { sim.run() })
            });
        }
        group.finish();
        Ok(())
    }

}

impl<T> RunConfig<T, BytecodeSelection> {

    pub fn set_bytecode_selection_opt(&mut self, selection: Option<BytecodeSelectionCli>) {
        if let Some(selection) = selection {
            self.set_bytecode_selection(selection);
        }
    }

    pub fn set_bytecode_selection(&mut self, selection: BytecodeSelectionCli) {
        self.compile_selection = match selection {
            BytecodeSelectionCli::Selected => BytecodeSelection::Selected,
            BytecodeSelectionCli::GasGuzzlers(config) => {
                let (config, size_limit) = config.into();
                BytecodeSelection::GasGuzzlers { config, size_limit }
            }
        };
    }

}

fn txs_for_block(provider_factory: &ProviderFactory<DatabaseEnv>, block_num: u64) -> Result<Vec<B256>> {
    let block = provider_factory.block(block_num.into())?
        .ok_or_eyre("Block not found")?;
    Ok(block.body.iter().map(|tx| tx.hash).collect())
}


#[derive(Debug, serde::Serialize)]
enum MeasureId {
    Block(u64),
    Tx(B256),
}

#[derive(Debug, serde::Serialize)]
struct MeasureRecord {
    id: MeasureId,
    run_type: String,
    exe_time: f64,
    err: Option<String>,
}

pub struct BlockRangeArgs {
    pub block_iter: Vec<u64>,
    pub out_path: PathBuf,
    pub warmup_ms: u32,
    pub measurement_ms: u32,
    pub block_chunk: Option<BlockPart>,
    pub run_rnd_txs: bool,
    pub seed: Option<[u8;32]>,
}