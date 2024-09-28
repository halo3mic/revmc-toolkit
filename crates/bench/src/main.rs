mod utils;
mod cli;
mod benches;

use std::{path::PathBuf, str::FromStr};
use revm::primitives::{B256, U256};
use tracing::{info, span, Level};
use cli::{Cli, Commands};
use eyre::{Ok, Result};
use clap::Parser;

use revmc_toolkit_utils::{evm as evm_utils, build as build_utils};
use revmc_toolkit_sim::sim_builder::BlockPart;
use utils::sim::{BytecodeSelection, SimRunType, SimCall};
use benches::BenchConfig;


fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv()?;

    let reth_db_path: PathBuf = std::env::var("RETH_DB_PATH")?.parse()?;
    let dir_path = revmc_toolkit_build::default_dir();

    let cli = Cli::parse();
    match cli.command {
        Commands::Build(_) => {
            // todo: into build utils
            let provider_factory = evm_utils::make_provider_factory(&reth_db_path)?;
            let state_provider = provider_factory.latest()?;
            let path = utils::default_build_config_path()?;
            info!("Compiling AOT from config file: {:?}", path);
            let span = span!(Level::INFO, "build");
            let _guard = span.enter();
            build_utils::compile_aot_from_file_path(&state_provider, &path)?
                .into_iter().collect::<Result<Vec<_>>>()?;
        }
        Commands::Run(run_args) => {
            let run_type = run_args.run_type.parse::<SimRunType>()?;
            info!("Running sim for type: {:?}", run_type);

            // if let Some(tx_hash) = run_args.tx_hash {
            //     let tx_hash = B256::from_str(&tx_hash)?;
            //     runners::run_tx(tx_hash, run_type, &BenchConfig::new(dir_path, reth_db_path, BytecodeSelection::Selected))?;
            // }

            // todo into call utils

            // let result = 
            //     if let Some(tx_hash) = run_args.tx_hash {
            //         let tx_hash = B256::from_str(&tx_hash)?;
            //         info!("Running sim for tx: {tx_hash:?}");
            //         sim::run_tx_sim(tx_hash, run_type, &config)?
            //     } else if let Some(block_num) = run_args.block_num {
            //         let block_num = block_num.parse::<u64>()?;
            //         let block_chunk = run_args.tob_block_chunk
            //             .map(|c| BlockPart::TOB(c))
            //             .or(run_args.bob_block_chunk.map(|c| BlockPart::BOB(c)));
            //         info!("Running sim for block: {block_num:?}");
            //         sim::run_block_sim(block_num, run_type, &config, block_chunk)?
            //     } else {
            //         let call_type = SimCall::Fibbonacci; // todo: different call opt
            //         info!("Running sim for call: {call_type:?}");
            //         sim::run_call_sim(call_type, run_type, &config)?
            //     };

            // todo: reimplement touches and success monitoring
            // result.contract_touches.into_iter().for_each(|(address, touch_counter)| {
            //     let revmc_toolkit_load::TouchCounter { non_native, overall } = touch_counter;
            //     if result.non_native_exe && non_native != overall {
            //         println!("{}/{} native touches for address {address:?}", overall-non_native, overall);
            //     } else if !result.non_native_exe && non_native != 0 {
            //         println!("{}/{} non-native touches for address {address:?}", non_native, overall);
            //     }
            // });
            // println!("Success: {:?}", result.success);
            // println!("Expected-gas-used: {:?} / Actual-gas-used: {:?}", result.expected_gas_used, result.gas_used);
        }
        Commands::Bench(bench_args) => {
            info!("Running benches");

            let config = BenchConfig::new(
                dir_path, 
                reth_db_path, 
                BytecodeSelection::Selected // todo: add opt for gas guzzlers
            );


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
                let input = U256::from(100_000).to_be_bytes_vec().into();
                info!("Running bench for call: {call_type:?}");
                benches::run_call_benchmarks(SimCall::Fibbonacci, input, &config)?;
            }
        }, 
        Commands::BlockRange(range_args) => {
            info!("Comparing block range: {}", range_args.block_range);

            let config = BenchConfig::new(
                dir_path, 
                reth_db_path, 
                BytecodeSelection::Selected // todo: add opt for gas guzzlers
            );


            let args = range_args.try_into()?;
            benches::compare_block_range(args, &config)?;
        }
    }
    Ok(())

}

