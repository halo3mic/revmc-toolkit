mod cli;

use clap::Parser;
use eyre::Result;
use revm::primitives::Bytecode;
use revmc_toolkit_sim::gas_guzzlers::{BytecodeStat, GasGuzzlerConfig};
use revmc_toolkit_utils::{
    self as utils,
    evm::{DatabaseEnv, ProviderFactory},
};
use std::path::Path;

fn main() -> Result<()> {
    let args = cli::Cli::parse();

    let gas_guzzlers = find_gas_guzzlers(args.start_block, args.end_block, args.sample_size)?;
    let parsed = parse_gas_guzzlers(gas_guzzlers, args.take, args.gas_limit);
    stdout(parsed, args.hashed)?;

    Ok(())
}

fn find_gas_guzzlers(
    start_block: u64,
    end_block: u64,
    sample_size: u64,
) -> Result<Vec<BytecodeStat<Bytecode>>> {
    let provider_factory = make_provider_factory()?;
    Ok(GasGuzzlerConfig::default()
        .with_start_block(start_block)
        .with_end_block(end_block)
        .with_sample_size(sample_size)
        .find_gas_guzzlers(provider_factory.clone())?
        .into_top_guzzlers_stats(None))
}

fn make_provider_factory() -> Result<ProviderFactory<DatabaseEnv>> {
    dotenv::dotenv()?;
    let db_path = std::env::var("RETH_DB_PATH")?;
    let db_path = Path::new(&db_path);
    let provider_factory = utils::evm::make_provider_factory(db_path)?;
    Ok(provider_factory)
}

fn parse_gas_guzzlers(
    gas_guzzlers: Vec<BytecodeStat<Bytecode>>,
    max_len: Option<usize>,
    max_csum_prop_gas_used: Option<f64>,
) -> Vec<BytecodeStat<Bytecode>> {
    let giter = gas_guzzlers.into_iter();

    match (max_len, max_csum_prop_gas_used) {
        (Some(max_len), Some(max_csum_prop_gas_used)) => giter
            .take(max_len)
            .take_while(|e| e.csum_prop_gas_used < max_csum_prop_gas_used)
            .collect(),
        (Some(max_len), None) => giter.take(max_len).collect(),
        (None, Some(max_csum_prop_gas_used)) => giter
            .take_while(|e| e.csum_prop_gas_used < max_csum_prop_gas_used)
            .collect(),
        (None, None) => giter.collect(),
    }
}

fn stdout(gas_guzzlers: Vec<BytecodeStat<Bytecode>>, hashed: bool) -> Result<()> {
    let str_out = if hashed {
        let gas_guzzlers: Vec<_> = gas_guzzlers
            .into_iter()
            .map(|e| e.bytecode_to_hash())
            .collect();
        serde_json::to_string_pretty(&gas_guzzlers)?
    } else {
        serde_json::to_string_pretty(&gas_guzzlers)?
    };
    println!("{str_out}");
    Ok(())
}
