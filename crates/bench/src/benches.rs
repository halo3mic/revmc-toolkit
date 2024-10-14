use std::{path::PathBuf, time::Duration};
use tracing::{info, warn, span, Level};
use criterion::Criterion;
use eyre::{OptionExt, Result};
use revm::primitives::B256;
use reth_provider::BlockReader;
use reth_primitives::Bytes;

use revmc_toolkit_utils::rnd as rnd_utils;
use crate::utils::sim::{SimCall, SimConfig, SimRunType, self as sim_utils};
use crate::cli::BytecodeSelectionCli;


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

    let provider_factory = make_provider_factory(&config.reth_db_path)?;

    let bytecodes = config.compile_selection.bytecodes(provider_factory.clone(), Some(vec![tx_hash]))?;

    // todo: this is also repeated
    for (symbol, run_type) in [
        ("aot", SimRunType::AOTCompiled),
        ("native", SimRunType::Native),
        // ("jit", SimRunType::JITCompiled),
    ] {
        info!("Running {}", symbol.to_uppercase());

        let ext_ctx = sim_utils::make_ext_ctx(run_type, bytecodes.clone(), Some(&config.dir_path))?;
        let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx) // todo: make arch optional?
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

    let provider_factory = make_provider_factory(&config.reth_db_path)?;
    let bytecodes = config.compile_selection.bytecodes(
        provider_factory.clone(),
        Some(txs_for_block(&provider_factory, block_num)?)
    )?;

    for (symbol, run_type) in [
        ("native", SimRunType::Native),
        // ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled),
    ] {
        info!("Running {}", symbol.to_uppercase());

        let ext_ctx = sim_utils::make_ext_ctx(run_type, bytecodes.clone(), Some(&config.dir_path))?;
        let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx)
            .make_block_sim(block_num, block_chunk)?;

        // check_fn_validity(&mut fnc)?;
        criterion.bench_function(&format!("sim_block_{symbol}"), |b| {
            b.iter(|| { sim.run() })
        });
    }
    Ok(())
}

pub fn run_call_benchmarks(call: SimCall, call_input: Bytes, config: &BenchConfig) -> Result<()> {
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
            Some(&config.dir_path)
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

use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use revmc_toolkit_utils::evm::make_provider_factory;
use revmc_toolkit_sim::sim_builder::BlockPart;
use crate::utils::sim::BytecodeSelection;
use revmc_toolkit_sim::bytecode_touches;
use reth_provider::ProviderFactory;
use reth_db::DatabaseEnv;

use crate::utils::bench as bench_utils;

pub(crate) struct BenchConfig {
    pub dir_path: PathBuf,
    pub reth_db_path: PathBuf,
    pub compile_selection: BytecodeSelection,
}

impl BenchConfig {
    pub fn new(
        dir_path: PathBuf, 
        reth_db_path: PathBuf, 
        compile_selection: BytecodeSelection
    ) -> Self {
        Self { dir_path, reth_db_path, compile_selection }
    }

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

pub fn compare_block_range(args: BlockRangeArgs, config: &BenchConfig) -> Result<()> {
    let provider_factory = make_provider_factory(&config.reth_db_path)?;
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
            ext_ctx = Some(sim_utils::make_ext_ctx(run_type.clone(), bytecodes, Some(&config.dir_path))?);
        }
        let measurements = block_iter.clone().into_par_iter().map(|block_num| {
            let mut ext_ctx = ext_ctx.clone();


            info!("Running {} for block {block_num}", symbol.to_uppercase());

            if let BytecodeSelection::Selected = config.compile_selection {
                let txs = txs_for_block(&provider_factory, block_num)?;
                let bytecodes = bytecode_touches::find_touched_bytecode(provider_factory.clone(), txs)?
                    .into_iter().collect();
                // todo: cached external ctx from previous blocks
                ext_ctx = Some(sim_utils::make_ext_ctx(run_type.clone(), bytecodes, Some(&config.dir_path))?);
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
}

pub struct BlockRangeArgs {
    pub block_iter: Vec<u64>,
    pub out_path: PathBuf,
    pub warmup_ms: u32,
    pub measurement_ms: u32,
    pub block_chunk: Option<BlockPart>,
    pub run_rnd_txs: bool,
}