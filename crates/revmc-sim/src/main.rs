


mod aot_evm;
mod build;
mod utils;
mod sim;
mod fn_loader;

use build::CompilerOptions;
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
    

    
    // Run
    
    // let univ2_router = address!("f164fC0Ec4E93095b804a4795bBe1e041497b92a");
    // let label = "univ2_router";
    // let state_provider = provider_factory.latest()?;
    // let code = state_provider.account_code(univ2_router)?
    //     .ok_or_eyre("No code found for address")?;
    // let spdb = StateProviderDatabase::new(state_provider);

    // let mut evm = evm::create_evm(vec![
    //     (label.to_string(), code.hash_slow()).into()
    // ], spdb)?;

    // evm.context.evm.env.tx.transact_to = TransactTo::Call(univ2_router);
    // evm.context.evm.env.tx.data = Bytes::from_str("0xd06ca61f0000000000000000000000000000000000000000000000000de0b6b3a764000000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000006982508145454ce325ddbe47a25d4ec3d2311933")?;
    // evm.context.evm.env.tx.gas_limit = 100_000;

    // match evm.transact() {
    //     Ok(res) =>  eprintln!("{:#?}", res.result),
    //     Err(e) => println!("error: {:?}", e),
    // }

    Ok(())
}

use revm::primitives::{BlockEnv, CfgEnvWithHandlerCfg, SpecId, CfgEnv};

fn run_tx_example() -> Result<()> {
    let db_path = std::env::var("RETH_DB_PATH")?;
    let provider_factory = utils::make_provider_factory(&db_path)?;

    let tx_hash = FixedBytes::<32>::from_str("0xe7bdb100811cdd8da59c8afa9999d49d3343dc90d2578cb3e9d3ff9fe26e34f9")?;
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
    let evm = aot_evm::create_evm(dir_path_str, None, db, env)?;



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

fn compile_example() -> Result<()> {
    let db_path = std::env::var("RETH_DB_PATH")?;
    let provider_factory = utils::make_provider_factory(&db_path)?;

    let contracts = vec![
        utils::CompileArgsWithAddress {
            address: address!("f164fC0Ec4E93095b804a4795bBe1e041497b92a"),
            options: Some(CompilerOptions::default().with_label("univ3_router"))
        },
        utils::CompileArgsWithAddress {
            address: address!("1111111254eeb25477b68fb85ed929f73a960582"),
            options: Some(CompilerOptions::default().with_label("1inch_v5"))
        },
        utils::CompileArgsWithAddress {
            address: address!("87870bca3f3fd6335c3f4ce8392d69350b4fa4e2"),
            options: Some(CompilerOptions::default().with_label("aave_v3")),
        },
    ];
    let state_provider = Arc::new(provider_factory.latest()?);
    utils::compile_contracts_with_address(state_provider, contracts, None)?;

    Ok(())
} 

