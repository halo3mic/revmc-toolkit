


mod utils;
mod sim;

use revmc_sim_build::CompilerOptions;
use reth_revm::database::StateProviderDatabase;
use revm::primitives::{address, Bytes, FixedBytes, TransactTo};

use eyre::{Result, OptionExt};
use std::str::FromStr;
use std::sync::Arc;

use reth_evm_ethereum::EthEvmConfig;
use reth_evm::ConfigureEvmEnv;

use reth_db::open_db_read_only;
use reth_provider::{
    providers::StaticFileProvider, BlockNumReader, BlockReader, ChainSpecProvider, EvmEnvProvider, ProviderFactory, ReceiptProvider, StateProvider, TransactionsProvider
};
use std::path::Path;
use revm::{Evm, Database, handler::register::EvmHandler, db::CacheDB, primitives::{EnvWithHandlerCfg, TxEnv}};
use reth_primitives::{Address, Bytecode as RethBytecode, B256};
use reth_chainspec::ChainSpecBuilder;


// todo: Tx simulation
// todo: Block simulation
// todo: compilation in a seperate crate + cli + json parsing of settings
// todo: benchmarking + set different compile options / tx_bench & block_bench

fn main() -> Result<()>{
    dotenv::dotenv().ok();

    // compile_example()?;
    
    run_tx_example()?;

    Ok(())
}

use revm::primitives::{BlockEnv, CfgEnvWithHandlerCfg, SpecId, CfgEnv};

fn run_tx_example() -> Result<()> {
    let db_path = std::env::var("RETH_DB_PATH")?;
    let provider_factory = utils::make_provider_factory(&db_path)?;

    let tx_hash = FixedBytes::<32>::from_str("0x1fe4ff2ef38d406d40dedd760a555120559866888715ff88f6cef90427c3c33b")?;
    let (tx, meta) = provider_factory.transaction_by_hash_with_meta(tx_hash)?
        .ok_or_eyre("No tx found")?;

    let block = provider_factory.block(meta.block_number.into())?.ok_or_eyre("No block found")?;

    // todo: do this myself?
    let mut block_env = BlockEnv::default();
    let eth_evm_cfg = EthEvmConfig::default();
    eth_evm_cfg.fill_block_env(
        &mut block_env,
        &block.header,
        block.header.number >= 15537393,
    );

    let chain_id = provider_factory.chain_spec().chain.id();
    let cfg_env = CfgEnvWithHandlerCfg::new_with_spec_id(CfgEnv::default().with_chain_id(chain_id), SpecId::CANCUN);
    let state_provider = provider_factory.history_by_block_number((meta.block_number-1).into())?;


    let db = CacheDB::new(StateProviderDatabase::new(state_provider));

    let env = EnvWithHandlerCfg::new_with_cfg_env(cfg_env, block_env, TxEnv::default());
    // let mut evm = revm::Evm::builder().with_db(db).with_env_with_handler_cfg(env).build(); // Normie evm

    let dir_path = std::env::current_dir()?.join(".data");
    let dir_path_str = dir_path.to_string_lossy().to_string();

    println!("creating evm");
    let evm = create_evm(dir_path_str, db, env, None)?;



    let res = sim::sim_txs(
        vec![tx], 
        evm,
    )?;

    println!("{:#?}", res);

    // todo: get all aotc contracts

    Ok(())
}

fn run_block_example() {

}

use revmc_sim_load::{ExternalContext, self as loader};

pub fn create_evm<ExtDB: revm::Database + revm::DatabaseRef>(
    dir_path: String,
    db: CacheDB<ExtDB>, 
    cfg_env: EnvWithHandlerCfg,
    codehash_select: Option<Vec<B256>>,
) -> Result<Evm<'static, ExternalContext, CacheDB<ExtDB>>> {
    let external_ctx = loader::build_external_context(dir_path, codehash_select)?;
    let evm = revm::Evm::builder()
        .with_db(db)
        .with_external_context(external_ctx)
        .with_env_with_handler_cfg(cfg_env)
        .append_handler_register(loader::register_handler)
        .build();
    Ok(evm)
}

