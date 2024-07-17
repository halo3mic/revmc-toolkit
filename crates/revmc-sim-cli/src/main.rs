mod utils;
mod cli;
mod sim;

use std::{time::Duration, str::FromStr, sync::Arc};
use criterion::Criterion;
use cli::{Cli, Commands};
use clap::Parser;
use eyre::Result;
use revm::primitives::B256;

use sim::{SimCall, SimConfig, SimRunType};


fn main() -> Result<()> {
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
            utils::build::compile_aot_from_file_path(&state_provider, &path)?
                .into_iter().collect::<Result<Vec<_>>>()?;
        }
        Commands::Run(run_args) => {
            let run_type = run_args.run_type.parse::<SimRunType>()?;
            println!("Run type: {:?}", run_type);
            if let Some(tx_hash) = run_args.tx_hash {
                let tx_hash = B256::from_str(&tx_hash)?;
                println!("Running sim for tx: {:?}", tx_hash);
                sim::run_tx_sim(tx_hash, run_type, &config)?;
            } else if let Some(block_num) = run_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                println!("Running sim for block: {block_num:?}");
                sim::run_block_sim(block_num, run_type, &config)?;
            } else {
                println!("Running sim for call"); // todo: different call opt
                sim::run_call_sim(SimCall::Fibbonacci, run_type, &config)?;
            }
        }
        Commands::Bench(bench_args) => {
            // todo: how many iters
            if let Some(tx_hash) = bench_args.tx_hash {
                let tx_hash = B256::from_str(&tx_hash)?;
                run_tx_benchmarks(tx_hash, &config)?;
            } else if let Some(block_num) = bench_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                run_block_benchmarks(block_num, &config)?;
            } else {
                // todo: call type in args
                run_call_benchmarks(SimCall::Fibbonacci, &config)?;
            }
        }
    }
    
    Ok(())

}

fn run_tx_benchmarks(tx_hash: B256, config: &SimConfig) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));

    let mut fn_jit = sim::make_tx_sim(tx_hash, SimRunType::JITCompiled, config)?;
    criterion.bench_function("sim_tx_jit_compiled", |b| {
        b.iter(|| { fn_jit() })
    });

    let mut fn_aot = sim::make_tx_sim(tx_hash, SimRunType::AOTCompiled, config)?;
    criterion.bench_function("sim_tx_aot_compiled", |b| {
        b.iter(|| { fn_aot() })
    });

    let mut fn_native = sim::make_tx_sim(tx_hash, SimRunType::Native, config)?;
    criterion.bench_function("sim_tx_native", |b| {
        b.iter(|| { fn_native() })
    });

    Ok(())
}

fn run_block_benchmarks(block_num: u64, config: &SimConfig) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(20));

    unimplemented!(); // todo: implement block benchmarks

    Ok(())
}

fn run_call_benchmarks(call: SimCall, config: &SimConfig) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));

    let mut fn_jit = sim::make_call_sim(call, SimRunType::JITCompiled, config)?;
    criterion.bench_function("sim_call_jit", |b| {
        b.iter(|| { fn_jit() })
    });

    let mut fn_aot = sim::make_call_sim(call, SimRunType::AOTCompiled, config)?;
    criterion.bench_function("sim_call_aot_compiled", |b| {
        b.iter(|| { fn_aot() })
    });

    let mut fn_native = sim::make_call_sim(call, SimRunType::Native, config)?;
    criterion.bench_function("sim_call_native", |b| {
        b.iter(|| { fn_native() })
    });

    Ok(())
}
