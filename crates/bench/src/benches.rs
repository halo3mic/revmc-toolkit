use std::{path::PathBuf, time::Duration};
use tracing::{info, warn, span, Level};
use criterion::Criterion;
use eyre::{OptionExt, Result};
use revm::primitives::B256;
use reth_provider::BlockReader;

use super::sim::{self, SimCall, SimConfig, SimRunType, BlockPart};
use super::utils;
use super::cli;


pub fn run_tx_benchmarks(tx_hash: B256, config: &SimConfig) -> Result<()> {
    let dir_path = PathBuf::from(config.dir_path.to_string());
    let span = span!(Level::INFO, "bench_tx");
    let _guard = span.enter();
    info!("TxHash: {:?}", tx_hash);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));

    for (symbol, run_type) in [
        ("aot", SimRunType::AOTCompiled { dir_path }),
        ("native", SimRunType::Native),
        // ("jit", SimRunType::JITCompiled),
    ] {
        info!("Running {}", symbol.to_uppercase());
        let mut fnc = sim::make_tx_sim(tx_hash, run_type, config)?;
        check_fn_validity(&mut fnc)?;
        criterion.bench_function(&format!("sim_tx_{symbol}"), |b| {
            b.iter(|| { fnc() })
        });
    }
    Ok(())
}

pub fn run_block_benchmarks(block_num: u64, config: &SimConfig, block_chunk: Option<BlockPart>) -> Result<()> {
    let dir_path = PathBuf::from(config.dir_path.to_string());
    let span = span!(Level::INFO, "bench_block");
    let _guard = span.enter();
    info!("Block: {:?}", block_num);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));

    for (symbol, run_type) in [
        ("native", SimRunType::Native),
        // ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled { dir_path }),
    ] {
        info!("Running {}", symbol.to_uppercase());
        let mut fnc = sim::make_block_sim(block_num, run_type, config, block_chunk)?;
        check_fn_validity(&mut fnc)?;
        criterion.bench_function(&format!("sim_block_{symbol}"), |b| {
            b.iter(|| { fnc() })
        });
    }
    Ok(())
}

pub fn run_call_benchmarks(call: SimCall, config: &SimConfig) -> Result<()> {
    let dir_path = PathBuf::from(config.dir_path.to_string());
    let span = span!(Level::INFO, "bench_call");
    let _guard = span.enter();
    info!("Call: {:?}", call);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));
    let mut group = criterion.benchmark_group("call_benchmarks");
    
    for (symbol, run_type) in [
        ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled { dir_path }),
        ("native", SimRunType::Native),
    ] {
        info!("Running {}", symbol.to_uppercase());
        let mut fnc = sim::make_call_sim(call, run_type, config)?;
        check_fn_validity(&mut fnc)?;
        group.bench_function(&format!("sim_call_{symbol}"), |b| {
            b.iter(|| { fnc() })
        });
    }
    group.finish();
    Ok(())
}

fn check_fn_validity(fnc: &mut Box<dyn FnMut() -> Result<crate::sim::SimExecutionResult>>) -> Result<()> {
    for _ in 0..3 {
        let result = fnc()?;
        if !result.gas_used_matches_expected() {
            return Err(eyre::eyre!(
                "Invalid gas used, expected: {:?}, actual: {:?}", 
                result.expected_gas_used, 
                result.gas_used
            ));
        }
        if let Some(wrong_touches) = result.wrong_touches() {
            warn!("Invalid touches for contracts {:?}", wrong_touches);
            // return Err(eyre::eyre!("Invalid touches for contracts {:?}", wrong_touches));
        }
        // todo: properly check result success for block eg hash of all ordered successes
        // if !result.success() {
        //     return Err(eyre::eyre!("Execution failed"));
        // }
    }
    Ok(())
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

pub fn compare_block_range(args: BlockRangeArgs, config: &SimConfig) -> Result<()> {
    let dir_path = PathBuf::from(config.dir_path.to_string());
    let mut writer = csv::WriterBuilder::new().from_path(&args.out_path)?;

    let span = span!(Level::INFO, "compare_block_range");
    let _guard = span.enter();

    for block_num in args.block_iter {
        for (symbol, run_type) in [
            ("native", SimRunType::Native),
            ("aot", SimRunType::AOTCompiled { dir_path: dir_path.clone() }),
            // ("jit", SimRunType::JITCompiled),
        ] {
            info!("Running {} for block {block_num}", symbol.to_uppercase());
            let run_type_clone = run_type.clone();
            let (mut fnc, m_id) = 
                if args.run_rnd_txs {
                    let block = config.provider_factory.block(block_num.into())?
                        .ok_or_eyre("Block not found")?;
                    if block.body.is_empty() {
                        warn!("Found empty block {}, skipping", block_num);
                        continue;
                    }
                    let txs_len = args.block_chunk
                        .map(|chunk| {
                            match chunk {
                                BlockPart::TOB(c) => (block.body.len() as f32 * c) as usize,
                                BlockPart::BOB(c) => (block.body.len() as f32 * (1. - c)) as usize,
                            }
                        })
                        .unwrap_or(block.body.len());
                    let tx_index = utils::rnd::random_sequence(0, txs_len, 1, Some([4;32]))?[0]; // todo: include tx-seed in config?
                    let tx_hash = block.body[tx_index].hash;
                    info!("Running random tx: {tx_hash:?}");
                    (sim::make_tx_sim(tx_hash, run_type_clone, config)?, MeasureId::Tx(tx_hash))
                } else {
                    (sim::make_block_sim(block_num, run_type_clone, config, args.block_chunk)?, MeasureId::Block(block_num))
                };
            check_fn_validity(&mut fnc)?;
            let exe_time = measure_execution_time(
                || { fnc() }, 
                args.warmup_ms, 
                args.measurement_ms
            );
            writer.serialize(MeasureRecord {
                run_type: symbol.to_string(),
                id: m_id,
                exe_time,
            })?;
        }
        writer.flush()?;
    }
    info!("Finished comparing block range ✨");
    info!("The records are written to {}", args.out_path.display());
    Ok(())
}

use std::time::Instant;

fn measure_execution_time<F, R>(mut f: F, warmup_ms: u32, measurement_ms: u32) -> f64
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

pub struct BlockRangeArgs {
    block_iter: Vec<u64>,
    out_path: PathBuf,
    warmup_ms: u32,
    measurement_ms: u32,
    block_chunk: Option<BlockPart>,
    run_rnd_txs: bool,
}

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
                utils::rnd::random_sequence(start, end, sample_size as usize, rnd_seed)?
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