use std::{path::PathBuf, time::Duration};
use tracing::{info, warn, span, Level};
use criterion::Criterion;
use eyre::{OptionExt, Result};
use revm::primitives::B256;
use reth_provider::BlockReader;
use reth_primitives::Bytes;

use revmc_toolbox_utils::rnd as rnd_utils;
use crate::utils::sim::{SimCall, SimConfig, SimRunType, self as sim_utils};


// todo: make bench config a struct
// todo: sample_size and measurement_time as args

// todo: reimplement verification

pub fn run_tx_benchmarks(tx_hash: B256, config: &BenchConfig) -> Result<()> {
    let span = span!(Level::INFO, "bench_tx");
    let _guard = span.enter();
    info!("TxHash: {:?}", tx_hash);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));

    let provider_factory = Arc::new(make_provider_factory(&config.reth_db_path)?);

    // todo: this is repated -> to utils
    let bytecodes = 
        match &config.compile_selection {
            BytecodeSelection::GasGuzzlers { config: gconfig, size_limit } => {
                gconfig.find_gas_guzzlers(provider_factory.clone())?
                    .contract_to_bytecode()?
                    .into_top_guzzlers(*size_limit)
            },
            BytecodeSelection::Selected => {
                bytecode_touches::find_touched_bytecode(provider_factory.clone(), vec![tx_hash])?
                    .into_iter().collect()
            }
        };

    // todo: this is also repeated
    for (symbol, run_type) in [
        ("aot", SimRunType::AOTCompiled),
        ("native", SimRunType::Native),
        // ("jit", SimRunType::JITCompiled),
    ] {
        info!("Running {}", symbol.to_uppercase());

        let ext_ctx = sim_utils::make_ext_ctx(run_type, bytecodes.clone(), Some(&config.dir_path))?;
        let mut sim = SimConfig::new(provider_factory.clone(), Arc::new(ext_ctx)) // todo: make arch optional?
            .make_tx_sim(tx_hash)?;

        // check_fn_validity(&mut fnc)?;
        criterion.bench_function(&format!("sim_tx_{symbol}"), |b| {
            b.iter(|| { sim.run() })
        });
    }
    Ok(())
}

pub fn run_block_benchmarks(block_num: u64, config: &BenchConfig, block_chunk: Option<BlockPart>) -> Result<()> {
    let span = span!(Level::INFO, "bench_block");
    let _guard = span.enter();
    info!("Block: {:?}", block_num);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));

    let provider_factory = Arc::new(make_provider_factory(&config.reth_db_path)?);
    let bytecodes = 
        match &config.compile_selection {
            BytecodeSelection::GasGuzzlers { config: gconfig, size_limit } => {
                gconfig.find_gas_guzzlers(provider_factory.clone())?
                    .contract_to_bytecode()?
                    .into_top_guzzlers(*size_limit)
            },
            BytecodeSelection::Selected => {
                let txs = txs_for_block(block_num, provider_factory.clone())?;
                bytecode_touches::find_touched_bytecode(provider_factory.clone(), txs)?
                    .into_iter().collect()
            }
        };

    for (symbol, run_type) in [
        ("native", SimRunType::Native),
        // ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled),
    ] {
        info!("Running {}", symbol.to_uppercase());

        let ext_ctx = sim_utils::make_ext_ctx(run_type, bytecodes.clone(), Some(&config.dir_path))?;
        let mut sim = SimConfig::new(provider_factory.clone(), Arc::new(ext_ctx)) // todo: make arch optional?
            .make_block_sim(block_num, block_chunk)?;

        // check_fn_validity(&mut fnc)?;
        criterion.bench_function(&format!("sim_block_{symbol}"), |b| {
            b.iter(|| { sim.run() })
        });
    }
    Ok(())
}

pub fn run_call_benchmarks(call: SimCall, call_input: Bytes, config: &BenchConfig) -> Result<()> {
    // todo: call bench not working properly

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

        let bytecode = call.bytecode().bytes().into();
        let ext_ctx = sim_utils::make_ext_ctx(
            run_type, 
            vec![bytecode], 
            Some(&config.dir_path)
        )?;
        let mut sim = SimConfig::from(Arc::new(ext_ctx))
            .make_call_sim(call, call_input.clone())?;

        // check_fn_validity(&mut fnc)?;
        group.bench_function(&format!("sim_call_{symbol}"), |b| {
            b.iter(|| { sim.run() })
        });
    }
    group.finish();
    Ok(())
}

// todo
// fn check_fn_validity(fnc: &mut Box<dyn FnMut() -> Result<Simulation>>) -> Result<()> {
//     for _ in 0..3 {
//         let result = fnc()?;
//         if !result.gas_used_matches_expected() {
//             return Err(eyre::eyre!(
//                 "Invalid gas used, expected: {:?}, actual: {:?}", 
//                 result.expected_gas_used, 
//                 result.gas_used
//             ));
//         }
//         if let Some(wrong_touches) = result.wrong_touches() {
//             warn!("Invalid touches for contracts {:?}", wrong_touches);
//             // return Err(eyre::eyre!("Invalid touches for contracts {:?}", wrong_touches));
//         }
//         // todo: properly check result success for block eg hash of all ordered successes
//         // if !result.success() {
//         //     return Err(eyre::eyre!("Execution failed"));
//         // }
//     }
//     Ok(())
// }

use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use revmc_toolbox_utils::evm::make_provider_factory;
use revmc_toolbox_sim::sim_builder::BlockPart;
use crate::utils::sim::BytecodeSelection;
use revmc_toolbox_sim::bytecode_touches;
use std::sync::Arc;
use reth_provider::ProviderFactory;
use reth_db::DatabaseEnv;

use crate::utils::bench as bench_utils;

pub struct BenchConfig {
    dir_path: PathBuf,
    reth_db_path: PathBuf,
    compile_selection: BytecodeSelection,
}

impl BenchConfig {
    pub fn new(
        dir_path: PathBuf, 
        reth_db_path: PathBuf, 
        compile_selection: BytecodeSelection
    ) -> Self {
        Self { dir_path, reth_db_path, compile_selection }
    }
}

pub fn compare_block_range(args: BlockRangeArgs, config: &BenchConfig) -> Result<()> {
    let provider_factory = Arc::new(make_provider_factory(&config.reth_db_path)?);
    let mut writer = csv::WriterBuilder::new().from_path(&args.out_path)?;

    let span = span!(Level::INFO, "compare_block_range");
    let _guard = span.enter();


    let block_iter = args.block_iter;
    for (symbol, run_type) in [
        ("native", SimRunType::Native),
        ("aot", SimRunType::AOTCompiled),
        // ("jit", SimRunType::JITCompiled),
    ] {
        let mut ext_ctx = None;
        if let BytecodeSelection::GasGuzzlers { config: gconfig, size_limit } = &config.compile_selection {
            let bytecodes = gconfig.find_gas_guzzlers(provider_factory.clone())?
                .contract_to_bytecode()?
                .into_top_guzzlers(*size_limit);
            ext_ctx = sim_utils::make_ext_ctx(run_type.clone(), bytecodes, Some(&config.dir_path))
                .map(|ctx| Some(Arc::new(ctx)))?;
        }
        let measurements = block_iter.clone().into_par_iter().map(|block_num| {
            let mut ext_ctx = ext_ctx.clone();


            info!("Running {} for block {block_num}", symbol.to_uppercase());

            if let BytecodeSelection::Selected = config.compile_selection {
                let txs = txs_for_block(block_num, provider_factory.clone())?;
                let bytecodes = bytecode_touches::find_touched_bytecode(provider_factory.clone(), txs)?
                    .into_iter().collect();
                // todo: cached external ctx from previous blocks
                ext_ctx = sim_utils::make_ext_ctx(run_type.clone(), bytecodes, Some(&config.dir_path))
                    .map(|ctx| Some(Arc::new(ctx)))?;
            }

            let sim_config = SimConfig::new(
                provider_factory.clone(), 
                ext_ctx.clone().expect("ExtCtx not found")
            );

            let (mut sim, m_id) = 
                if args.run_rnd_txs {
                    // todo: move this somewhere else
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
                    let tx_index = rnd_utils::random_sequence(0, txs_len, 1, Some([4;32]))?[0]; // todo: include tx-seed in config?
                    let tx_hash = block.body[tx_index].hash;
                    info!("Running random tx: {tx_hash:?}");

                    (sim_config.make_tx_sim(tx_hash)?, MeasureId::Tx(tx_hash))
                } else {
                    (sim_config.make_block_sim(block_num, args.block_chunk)?, MeasureId::Block(block_num))
                };
            // check_fn_validity(&mut fnc)?;
            let exe_time = bench_utils::measure_execution_time(
                || { sim.run() }, 
                args.warmup_ms, 
                args.measurement_ms
            );
            Ok(Some(MeasureRecord {
                run_type: symbol.to_string(),
                id: m_id,
                exe_time,
            }))
        }).collect::<Result<Vec<_>>>()?;
        // todo: uneccessary collecting + one err can ruin the whole thing
        for m in measurements {
            if let Some(record) = m {
                writer.serialize(record)?;
            }
        }
        writer.flush()?;
    }

    info!("Finished comparing block range ✨");
    info!("The records are written to {}", args.out_path.display());
    Ok(())
}

fn txs_for_block(block_num: u64, provider_factory: Arc<ProviderFactory<DatabaseEnv>>) -> Result<Vec<B256>> {
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
}

pub struct BlockRangeArgs {
    block_iter: Vec<u64>,
    out_path: PathBuf,
    warmup_ms: u32,
    measurement_ms: u32,
    block_chunk: Option<BlockPart>,
    run_rnd_txs: bool,
}

use crate::cli;
use crate::utils;

impl TryFrom<cli::BlockRangeArgsCli> for BlockRangeArgs {
    type Error = eyre::Error;

    // todo: declare default as constants
    fn try_from(cli_args: cli::BlockRangeArgsCli) -> Result<Self, Self::Error> {
        let [start, end, ..] = cli_args.block_range
            .split_terminator("..")
            .collect::<Vec<_>>()[..]
            else {
                return Err(eyre::eyre!("Invalid block range format"));
            };
        let start = start.parse::<u64>()?;
        let end = end.parse::<u64>()?;
        if end < start {
            return Err(eyre::eyre!("End block must be greater than start block"));
        }
        let default_out_dir = std::env::current_dir()?
            .join(".data/measurements");
        utils::make_dir(&default_out_dir)?;
        // todo: instead of epoch choose more representable label
        let label = cli_args.label.unwrap_or(format!("block_range_{}", utils::epoch_now()?));
        let out_path = cli_args.out_dir
            .map(|dir_path_str| PathBuf::from(dir_path_str))
            .unwrap_or(default_out_dir)
            .join(label + ".csv");
        let warmup_ms = cli_args.warmup_ms.unwrap_or(3_000);
        let measurement_ms = cli_args.measurement_ms.unwrap_or(5_000);
        let range_size = (end-start) as u32;
        let block_iter = 
            if let Some(sample_size) = cli_args.sample_size {
                if sample_size > range_size {
                    return Err(eyre::eyre!("Invalid sample size"));
                }
                let rnd_seed = cli_args.rnd_seed.map(|seed| revm::primitives::keccak256(seed.as_bytes()).0); // todo: instead of param into env
                rnd_utils::random_sequence(start, end, sample_size as usize, rnd_seed)?
            } else {
                (start..end).collect()
            };
        let block_chunk = 
            if let Some(tob) = cli_args.tob_block_chunk {
                Some(BlockPart::TOB(tob))
            } else if let Some(bob) = cli_args.bob_block_chunk {
                Some(BlockPart::BOB(bob))
            } else {
                None
            };

        Ok(Self {
            block_chunk,
            run_rnd_txs: cli_args.run_rnd_txs,
            block_iter,
            out_path,
            warmup_ms,
            measurement_ms,
        })
    }
}