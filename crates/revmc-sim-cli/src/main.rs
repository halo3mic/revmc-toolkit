mod utils;
mod cli;
mod sim;
mod benches;

use std::{str::FromStr, sync::Arc};
use tracing::{info, span, Level};
use cli::{Cli, Commands};
use clap::Parser;
use eyre::{Ok, Result};
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
            println!("Success: {:?}", result.success);
            println!("Expected-gas-used: {:?} / Actual-gas-used: {:?}", result.expected_gas_used, result.gas_used);
        }
        Commands::Bench(bench_args) => {
            info!("Running benches");
            // todo: how many iters
            if let Some(tx_hash) = bench_args.tx_hash {
                let tx_hash = B256::from_str(&tx_hash)?;
                info!("Running bench for tx: {tx_hash:?}");
                benches::run_tx_benchmarks(tx_hash, &config)?;
            } else if let Some(block_num) = bench_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                let block_chunk = bench_args.tob_block_chunk
                    .map(|c| BlockPart::TOB(c))
                    .or(bench_args.bob_block_chunk.map(|c| BlockPart::BOB(c)));
                info!("Running bench for block: {block_num:?}");
                benches::run_block_benchmarks(block_num, &config, block_chunk)?;
            } else {
                let call_type = SimCall::Fibbonacci; // todo: different call opt
                info!("Running bench for call: {call_type:?}");
                benches::run_call_benchmarks(SimCall::Fibbonacci, &config)?;
            }
        }, 
        Commands::BlockRange(range_args) => {
            info!("Comparing block range: {}", range_args.block_range);
            let args = range_args.try_into()?;
            benches::compare_block_range(args, &config)?;
        }
    }
    Ok(())

}

