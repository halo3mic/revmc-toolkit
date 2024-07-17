mod utils;
mod cli;
mod sim;

use std::{time::Duration, str::FromStr, sync::Arc};
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
    let dir_path = std::env::current_dir()?.join(".data");
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
        }
    }
    Ok(())

}

fn run_tx_benchmarks(tx_hash: B256, config: &SimConfig) -> Result<()> {
    let span = span!(Level::INFO, "bench_tx");
    let _guard = span.enter();
    info!("TxHash: {:?}", tx_hash);
    let mut criterion = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));

    for (symbol, run_type) in [
        ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled),
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
        .sample_size(10)
        .measurement_time(Duration::from_secs(5));

    for (symbol, run_type) in [
        ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled),
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
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));
    let mut group = criterion.benchmark_group("call_benchmarks");
    
    for (symbol, run_type) in [
        ("jit", SimRunType::JITCompiled),
        ("aot", SimRunType::AOTCompiled),
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
