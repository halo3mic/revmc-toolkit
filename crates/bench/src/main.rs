mod utils;
mod cli;
mod benches;
mod runners;

use std::{path::PathBuf, str::FromStr};
use revm::primitives::B256;
use tracing::info;
use cli::{Cli, Commands, BenchType, RunArgsCli, BlockArgsCli};
use eyre::{Ok, Result};
use clap::Parser;

use revmc_toolkit_sim::sim_builder::BlockPart;
use utils::{sim::{BytecodeSelection, SimCall}, bench::RunConfig};


fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    dotenv::dotenv()?;

    let reth_db_path: PathBuf = std::env::var("RETH_DB_PATH")?.parse()?;
    let dir_path = revmc_toolkit_build::default_dir();

    let cli = Cli::parse();
    match cli.command {
        Commands::Run(run_args) => {
            let mut config = RunConfig::new(dir_path, reth_db_path, BytecodeSelection::Selected);
            
            match run_args {
                RunArgsCli::Tx { tx_hash, run_type, bytecode_selection, comp_opt_level } => {
                    config.set_bytecode_selection_opt(bytecode_selection);
                    config.set_compile_opt_level(comp_opt_level);
                    let tx_hash = B256::from_str(&tx_hash)?;
                    info!("Running sim for tx: {tx_hash:?}");
                    config.run_tx(tx_hash, run_type.parse()?)?;
                }
                RunArgsCli::Block { run_type, block_args, bytecode_selection, comp_opt_level } => {
                    config.set_bytecode_selection_opt(bytecode_selection);
                    config.set_compile_opt_level(comp_opt_level);
                    let BlockArgsCli { block_num, tob_block_chunk, bob_block_chunk } = block_args;
                    let block_chunk = tob_block_chunk
                        .map(|c| BlockPart::TOB(c))
                        .or(bob_block_chunk.map(|c| BlockPart::BOB(c)));
                    info!("Running sim for block: {block_num:?}");
                    config.run_block(block_num, run_type.parse()?, block_chunk)?;
                }
                RunArgsCli::Call { input, run_type, comp_opt_level } => {
                    config.set_compile_opt_level(comp_opt_level);
                    let call_type = SimCall::Fibbonacci;
                    let input = input.unwrap_or(call_type.default_input());
                    info!("Running sim for call: {call_type:?} with input: {input:?}");
                    config.run_call(
                        call_type, 
                        input,
                        run_type.parse()?,
                    )?;
                }
            }
        }
        Commands::Bench(bench_args) => {
            let mut config = RunConfig::new(
                dir_path, 
                reth_db_path, 
                BytecodeSelection::Selected
            );
            
            match bench_args {
                BenchType::Tx { tx_hash, bytecode_selection, comp_opt_level } => {
                    info!("Running bench for tx: {tx_hash:?}");
                    config.set_bytecode_selection_opt(bytecode_selection);
                    config.set_compile_opt_level(comp_opt_level);
                    let tx_hash = B256::from_str(&tx_hash)?;
                    config.bench_tx(tx_hash)?;
                }
                BenchType::Block { block_args, bytecode_selection, comp_opt_level } => {
                    let BlockArgsCli { block_num, tob_block_chunk, bob_block_chunk } = block_args;
                    info!("Running bench for block: {:?}", block_num);
                    config.set_bytecode_selection_opt(bytecode_selection);
                    config.set_compile_opt_level(comp_opt_level);
                    let block_chunk = tob_block_chunk
                        .map(|c| BlockPart::TOB(c))
                        .or(bob_block_chunk.map(|c| BlockPart::BOB(c)));
                    config.bench_block(block_num, block_chunk)?;
                }
                BenchType::Call { comp_opt_level } => {
                    config.set_compile_opt_level(comp_opt_level);
                    let call_type = SimCall::Fibbonacci; // todo: different call opt
                    info!("Running bench for call: {call_type:?}");
                    let input = call_type.default_input();
                    config.bench_call(SimCall::Fibbonacci, input)?;
                }
                BenchType::BlockRange { block_range_args, bytecode_selection, comp_opt_level } => {
                    info!("Comparing block range: {}", block_range_args.block_range);
                    config.set_bytecode_selection_opt(bytecode_selection);
                    config.set_compile_opt_level(comp_opt_level);
                    let args = block_range_args.try_into()?;
                    config.bench_block_range(args)?;
                }
            }
        }
    }
    Ok(())

}

