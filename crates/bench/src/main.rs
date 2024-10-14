mod utils;
mod cli;
mod benches;
mod runners;

use std::{path::PathBuf, str::FromStr};
use revm::primitives::B256;
use tracing::{info, span, Level};
use cli::{Cli, Commands, BenchType, RunArgsCli, BlockArgsCli};
use eyre::{Ok, Result};
use clap::Parser;

use revmc_toolkit_utils::{evm as evm_utils, build as build_utils};
use revmc_toolkit_sim::sim_builder::BlockPart;
use utils::sim::{BytecodeSelection, SimCall};
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
            let config = BenchConfig::new(dir_path, reth_db_path, BytecodeSelection::Selected);
            
            match run_args {
                RunArgsCli::Tx { tx_hash, run_type } => {
                    let tx_hash = B256::from_str(&tx_hash)?;
                    info!("Running sim for tx: {tx_hash:?}");
                    runners::run_tx(tx_hash, run_type.parse()?, &config)?;
                }
                RunArgsCli::Block { run_type, block_args } => {
                    let BlockArgsCli { block_num, tob_block_chunk, bob_block_chunk } = block_args;
                    let block_chunk = tob_block_chunk
                        .map(|c| BlockPart::TOB(c))
                        .or(bob_block_chunk.map(|c| BlockPart::BOB(c)));
                    info!("Running sim for block: {block_num:?}");
                    runners::run_block(block_num, run_type.parse()?, &config, block_chunk)?;
                }
                RunArgsCli::Call { input, run_type } => {
                    let call_type = SimCall::Fibbonacci;
                    let input = input.unwrap_or(call_type.default_input());
                    info!("Running sim for call: {call_type:?} with input: {input:?}");
                    runners::run_call(
                        call_type, 
                        input,
                        run_type.parse()?,
                        Some(&config.dir_path)
                    )?;
                }
            }
        }
        Commands::Bench(bench_args) => {
            let config = BenchConfig::new(
                dir_path, 
                reth_db_path, 
                BytecodeSelection::Selected // todo: add opt for gas guzzlers
            );
            
            match bench_args {
                BenchType::Tx { tx_hash } => {
                    info!("Running bench for tx: {tx_hash:?}");
                    let tx_hash = B256::from_str(&tx_hash)?;
                    benches::run_tx_benchmarks(tx_hash, &config)?;
                }
                BenchType::Block(block_args) => {
                    let BlockArgsCli { block_num, tob_block_chunk, bob_block_chunk } = block_args;
                    info!("Running bench for block: {:?}", block_num);
                    let block_chunk = tob_block_chunk
                        .map(|c| BlockPart::TOB(c))
                        .or(bob_block_chunk.map(|c| BlockPart::BOB(c)));
                    benches::run_block_benchmarks(block_num, &config, block_chunk)?;
                }
                BenchType::Call => {
                    let call_type = SimCall::Fibbonacci; // todo: different call opt
                    info!("Running bench for call: {call_type:?}");
                    let input = call_type.default_input();
                    benches::run_call_benchmarks(SimCall::Fibbonacci, input, &config)?;
                }
                BenchType::BlockRange(range_args) => {
                    info!("Comparing block range: {}", range_args.block_range);
                    let args = range_args.try_into()?;
                    benches::compare_block_range(args, &config)?;
                }
            }
        }
    }
    Ok(())

}

