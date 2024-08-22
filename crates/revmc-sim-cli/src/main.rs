mod utils;
mod cli;
mod sim;

use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use reth_provider::BlockReader;
use tracing::{info, span, warn, Level};
use criterion::Criterion;
use cli::{Cli, Commands};
use clap::Parser;
use eyre::{OptionExt, Result};
use revm::primitives::B256;

use sim::{SimCall, SimConfig, SimRunType, BlockPart};


fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv()?;

    let db_path = std::env::var("RETH_DB_PATH")?;
    let dir_path = revmc_sim_build::default_dir();
    let dir_path = dir_path.to_string_lossy().to_string();
    let provider_factory = Arc::new(utils::evm::make_provider_factory(&db_path)?);
    let config = SimConfig::new(provider_factory, dir_path);

    let cli = Cli::parse();
    match cli.command {
        Commands::Build(_) => {
            // todo: parse config path
            let state_provider = config.provider_factory.latest()?;
            let path = utils::default_build_config_path()?;
            info!("Compiling AOT from config file: {:?}", path);
            let span = span!(Level::INFO, "build");
            let _guard = span.enter();
            utils::build::compile_aot_from_file_path(&state_provider, &path)?
                .into_iter().collect::<Result<Vec<_>>>()?;
        }
        Commands::Run(run_args) => {
            let run_type = run_args.run_type.parse::<SimRunType>()?;
            info!("Running sim for type: {:?}", run_type);
            let result = 
                if let Some(tx_hash) = run_args.tx_hash {
                    let tx_hash = B256::from_str(&tx_hash)?;
                    info!("Running sim for tx: {tx_hash:?}");
                    sim::run_tx_sim(tx_hash, run_type, &config)?
                } else if let Some(block_num) = run_args.block_num {
                    let block_num = block_num.parse::<u64>()?;
                    let block_chunk = run_args.tob_block_chunk
                        .map(|c| BlockPart::TOB(c))
                        .or(run_args.bob_block_chunk.map(|c| BlockPart::BOB(c)));
                    info!("Running sim for block: {block_num:?}");
                    sim::run_block_sim(block_num, run_type, &config, block_chunk)?
                } else {
                    let call_type = SimCall::Fibbonacci; // todo: different call opt
                    info!("Running sim for call: {call_type:?}");
                    sim::run_call_sim(call_type, run_type, &config)?
                };
            result.contract_touches.into_iter().for_each(|(address, touch_counter)| {
                let revmc_sim_load::TouchCounter { non_native, overall } = touch_counter;
                if result.non_native_exe && non_native != overall {
                    println!("{}/{} native touches for address {address:?}", overall-non_native, overall);
                } else if !result.non_native_exe && non_native != 0 {
                    println!("{}/{} non-native touches for address {address:?}", non_native, overall);
                }
            });
            println!("Success: {}", result.success);
            println!("Expected-gas-used: {} / Actual-gas-used: {}", result.expected_gas_used, result.gas_used);
        }
        Commands::Bench(bench_args) => {
            info!("Running benches");
            // todo: how many iters
            if let Some(tx_hash) = bench_args.tx_hash {
                let tx_hash = B256::from_str(&tx_hash)?;
                info!("Running bench for tx: {tx_hash:?}");
                run_tx_benchmarks(tx_hash, &config)?;
            } else if let Some(block_num) = bench_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                let block_chunk = bench_args.tob_block_chunk
                    .map(|c| BlockPart::TOB(c))
                    .or(bench_args.bob_block_chunk.map(|c| BlockPart::BOB(c)));
                info!("Running bench for block: {block_num:?}");
                run_block_benchmarks(block_num, &config, block_chunk)?;
            } else {
                let call_type = SimCall::Fibbonacci; // todo: different call opt
                info!("Running bench for call: {call_type:?}");
                run_call_benchmarks(SimCall::Fibbonacci, &config)?;
            }
        }, 
        Commands::BlockRange(range_args) => {
            info!("Comparing block range: {}", range_args.block_range);
            let args: BlockRangeArgs = range_args.try_into()?;
            compare_block_range(args, &config)?;
        }
    }
    Ok(())

}


fn run_tx_benchmarks(tx_hash: B256, config: &SimConfig) -> Result<()> {
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

fn run_block_benchmarks(block_num: u64, config: &SimConfig, block_chunk: Option<BlockPart>) -> Result<()> {
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

fn run_call_benchmarks(call: SimCall, config: &SimConfig) -> Result<()> {
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

fn compare_block_range(args: BlockRangeArgs, config: &SimConfig) -> Result<()> {
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
                    let tx_index = random_sequence(0, txs_len, 1, Some([4;32]))?[0]; // todo: include tx-seed in config?
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

fn epoch_now() -> Result<u64> {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    Ok(epoch)
}

struct BlockRangeArgs {
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
        make_dir(&default_out_dir)?;
        // todo: instead of epoch choose more representable label
        let label = cli_args.label.unwrap_or(format!("block_range_{}", epoch_now()?));
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
                random_sequence(start, end, sample_size as usize, rnd_seed)?
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

pub fn make_dir(dir_path: &PathBuf) -> Result<()> {
    if !dir_path.exists() {
        std::fs::create_dir_all(&dir_path)?;
    }
    Ok(())
}

use rand_chacha::ChaCha8Rng;
use rand::seq::SliceRandom;
use rand::SeedableRng;
use std::ops::Range;

// todo: could be very inefficient not to leave it as iter
fn random_sequence<T>(start: T, end: T, size: usize, seed: Option<[u8; 32]>) -> Result<Vec<T>>
where
    T: Copy + PartialOrd,
    Range<T>: Iterator<Item = T>,
    Vec<T>: FromIterator<<Range<T> as Iterator>::Item>,
{
    let mut rng = if let Some(seed) = seed {
        ChaCha8Rng::from_seed(seed)
    } else {
        ChaCha8Rng::from_entropy()
    };

    let range: Vec<T> = (start..end).collect();
    let mut shuffled = range;
    shuffled.shuffle(&mut rng);
    
    Ok(shuffled.into_iter().take(size).collect())
}

fn check_fn_validity(fnc: &mut Box<dyn FnMut() -> Result<crate::sim::SimExecutionResult>>) -> Result<()> {
    for _ in 0..3 {
        let result = fnc()?;
        if !result.gas_used_matches_expected() {
            return Err(eyre::eyre!(
                "Invalid gas used, expected: {}, actual: {}", 
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