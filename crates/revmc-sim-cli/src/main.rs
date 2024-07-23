mod utils;
mod cli;
mod sim;

use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use tracing::{info, span, Level};
use criterion::Criterion;
use cli::{Cli, Commands};
use clap::Parser;
use eyre::Result;
use revm::primitives::B256;

use sim::{SimCall, SimConfig, SimRunType};


fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv().ok();

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
            if let Some(tx_hash) = run_args.tx_hash {
                let tx_hash = B256::from_str(&tx_hash)?;
                info!("Running sim for tx: {tx_hash:?}");
                sim::run_tx_sim(tx_hash, run_type, &config)?;
            } else if let Some(block_num) = run_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                info!("Running sim for block: {block_num:?}");
                sim::run_block_sim(block_num, run_type, &config)?;
            } else {
                let call_type = SimCall::Fibbonacci; // todo: different call opt
                info!("Running sim for call: {call_type:?}");
                sim::run_call_sim(call_type, run_type, &config)?;
            }
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
                info!("Running bench for block: {block_num:?}");
                run_block_benchmarks(block_num, &config)?;
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
    let span = span!(Level::INFO, "bench_tx");
    let _guard = span.enter();
    info!("TxHash: {:?}", tx_hash);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));

    for (symbol, run_type) in [
        ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled { dir_path: config.dir_path.clone() }),
        ("native", SimRunType::Native),
    ] {
        info!("Running {}", symbol.to_uppercase());
        let mut fnc = sim::make_tx_sim(tx_hash, run_type, config)?;
        criterion.bench_function(&format!("sim_tx_{symbol}"), |b| {
            b.iter(|| { fnc() })
        });
    }
    Ok(())
}

fn run_block_benchmarks(block_num: u64, config: &SimConfig) -> Result<()> {
    let span = span!(Level::INFO, "bench_block");
    let _guard = span.enter();
    info!("Block: {:?}", block_num);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));

    for (symbol, run_type) in [
        ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled { dir_path: config.dir_path.clone() }),
        ("native", SimRunType::Native),
    ] {
        info!("Running {}", symbol.to_uppercase());
        let mut fnc = sim::make_block_sim(block_num, run_type, config)?;
        criterion.bench_function(&format!("sim_block_{symbol}"), |b| {
            b.iter(|| { fnc() })
        });
    }
    Ok(())
}

fn run_call_benchmarks(call: SimCall, config: &SimConfig) -> Result<()> {
    let span = span!(Level::INFO, "bench_call");
    let _guard = span.enter();
    info!("Call: {:?}", call);
    let mut criterion = Criterion::default()
        .sample_size(100)
        .measurement_time(Duration::from_secs(5));
    let mut group = criterion.benchmark_group("call_benchmarks");
    
    for (symbol, run_type) in [
        ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled { dir_path: config.dir_path.clone() }),
        ("native", SimRunType::Native),
    ] {
        info!("Running {}", symbol.to_uppercase());
        let mut fnc = sim::make_call_sim(call, run_type, config)?;
        group.bench_function(&format!("sim_call_{symbol}"), |b| {
            b.iter(|| { fnc() })
        });
    }
    group.finish();
    Ok(())
}

#[derive(Debug, serde::Serialize)]
struct MeasureRecord {
    block_num: u64,
    run_type: String,
    exe_time: f64,
}

fn compare_block_range(block_range: BlockRangeArgs, config: &SimConfig) -> Result<()> {
    let mut writer = csv::WriterBuilder::new().from_path(&block_range.out_path)?;

    let span = span!(Level::INFO, "compare_block_range");
    let _guard = span.enter();

    for block_num in block_range.block_iter {
        for (symbol, run_type) in [
            ("jit", SimRunType::JITCompiled),
            ("aot", SimRunType::AOTCompiled { dir_path: config.dir_path.clone() }),
            ("native", SimRunType::Native),
        ] {
            info!("Running {} for block {block_num}", symbol.to_uppercase());
            let run_type_clone = run_type.clone();
            let mut fnc = sim::make_block_sim(block_num, run_type_clone, config)?;
            let exe_time = measure_execution_time(
                || { let _ = fnc(); }, 
                block_range.warmup_iter, 
                block_range.bench_iter
            );
            writer.serialize(MeasureRecord {
                block_num,
                run_type: symbol.to_string(),
                exe_time,
            })?;
        }
    }
    writer.flush()?;
    info!("Finished comparing block range ✨");
    info!("The records are written to {}", block_range.out_path.display());
    Ok(())
}

use std::time::Instant;

fn measure_execution_time<F>(mut f: F, warm_up_iterations: u32, measured_iterations: u32) -> f64
where
    F: FnMut(),
{
    // Warm-up phase
    info!("Warming up with {warm_up_iterations} iterations");
    for _ in 0..warm_up_iterations {
        f();
    }

    // Measurement phase
    info!("Measuring with {measured_iterations} iterations");
    let mut total_duration = 0_u128;
    for _ in 0..measured_iterations {
        let start = Instant::now();
        f();
        let duration = Instant::now() - start;
        total_duration += duration.as_nanos();
    }

    total_duration as f64 / measured_iterations as f64
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
    warmup_iter: u32,
    bench_iter: u32,
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
            .join(".data/measurments");
        make_dir(&default_out_dir)?;
        let label = cli_args.label.unwrap_or(format!("block_range_{}", epoch_now()?));
        let out_path = cli_args.out_dir
            .map(|dir_path_str| PathBuf::from(dir_path_str))
            .unwrap_or(default_out_dir)
            .join(label + ".csv");
        let warmup_iter = cli_args.warmup_iter.unwrap_or(50_000);
        let bench_iter = cli_args.bench_iter.unwrap_or(100_000);
        let range_size = (end-start) as u32;
        let sample_size = cli_args.sample_size.unwrap_or(range_size);
        if sample_size > range_size {
            return Err(eyre::eyre!("Invalid sample size"));
        }
        let block_iter = random_sequence(start, end, sample_size as usize);

        Ok(Self {
            block_iter,
            out_path,
            warmup_iter,
            bench_iter,
        })
    }
}

pub fn make_dir(dir_path: &PathBuf) -> Result<()> {
    if !dir_path.exists() {
        std::fs::create_dir_all(&dir_path)?;
    }
    Ok(())
}

use rand::seq::SliceRandom;

// todo: could be very inefficient not to leave it as iter
fn random_sequence(start: u64, end: u64, size: usize) -> Vec<u64> {
    let mut rng = rand::thread_rng();
    let mut range: Vec<u64> = (start..end).collect();
    range.shuffle(&mut rng);
    range.into_iter().take(size).into_iter().collect()
}