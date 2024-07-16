mod utils;
mod sim;
mod cli;

use reth_provider::StateProvider;
use revm::{
    db::CacheDB, 
    primitives::{
        EnvWithHandlerCfg, TxEnv, BlockEnv, CfgEnvWithHandlerCfg, 
        SpecId, CfgEnv, B256, Address
    }
};
use reth_provider::{BlockReader, ChainSpecProvider, TransactionsProvider};
use reth_revm::database::StateProviderDatabase;
use reth_evm_ethereum::EthEvmConfig;
use reth_provider::ProviderFactory;
use reth_evm::ConfigureEvmEnv;
use reth_primitives::Block;
use reth_db::DatabaseEnv;

use eyre::{Result, OptionExt};
use std::time::Duration;
use std::str::FromStr;
use criterion::Criterion;

use cli::{Cli, Commands};
use std::sync::Arc;
use clap::Parser;

use revmc::primitives::hex;
use revm::primitives::{AccountInfo, Bytecode, TransactTo, address, U256, ExecutionResult};

struct Config {
    provider_factory: Arc<ProviderFactory<DatabaseEnv>>,
    dir_path: String,
}

const DEFAULT_BUILD_CONFIG: &str = "revmc.build.config.json";

fn default_build_config_path() -> Result<std::path::PathBuf> {
    Ok(std::env::current_dir()?.join(DEFAULT_BUILD_CONFIG))
}

fn main() -> Result<()> {
    dotenv::dotenv().ok();
    let db_path = std::env::var("RETH_DB_PATH")?;
    let dir_path = std::env::current_dir()?.join(".data");
    let dir_path = dir_path.to_string_lossy().to_string();
    let provider_factory = Arc::new(utils::evm::make_provider_factory(&db_path)?);


    let cli = Cli::parse();
    match cli.command {
        Commands::Build(_) => {
            // todo: parse config path
            let state_provider = provider_factory.latest()?;
            let path = default_build_config_path()?;
            utils::build::compile_aot_from_file_path(&state_provider, &path)?
                .into_iter().collect::<Result<Vec<_>>>()?;
        }
        Commands::Run(run_args) => {
            let run_type = run_args.run_type.parse::<RunType>()?;
            println!("Run type: {:?}", run_type);
            if let Some(tx_hash) = run_args.tx_hash {
                let tx_hash = B256::from_str(&tx_hash)?;
                println!("Running sim for tx: {:?}", tx_hash);
                run_tx_sim(tx_hash, run_type, Config { dir_path, provider_factory })?;
            } else if let Some(block_num) = run_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                println!("Running sim for block: {block_num:?}");
                run_block_sim(block_num, run_type)?;
            } else {
                // todo: format
                run_call_sim(Call::Fibbonacci, run_type, Config { dir_path, provider_factory })?;
                return Err(eyre::eyre!("Please provide either a transaction hash or a block number."));
            }
        }
        Commands::Bench(bench_args) => {
            // todo: how many iters
            if let Some(tx_hash) = bench_args.tx_hash {
                let tx_hash = B256::from_str(&tx_hash)?;
                run_tx_benchmarks(tx_hash, Config { dir_path, provider_factory })?;
            } else if let Some(block_num) = bench_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                run_block_benchmarks(block_num)?;
            } else {
                // todo: call type in args
                run_call_benchmarks(Call::Fibbonacci, Config { dir_path, provider_factory })?;
            }
        }
    }
    
    Ok(())

}

fn run_tx_benchmarks(tx_hash: B256, config: Config) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));

    let mut fn_jit = make_tx_sim(tx_hash, RunType::JITCompiled, &config)?;
    criterion.bench_function("sim_tx_jit_compiled", |b| {
        b.iter(|| { fn_jit() })
    });

    let mut fn_aot = make_tx_sim(tx_hash, RunType::AOTCompiled, &config)?;
    criterion.bench_function("sim_tx_aot_compiled", |b| {
        b.iter(|| { fn_aot() })
    });

    let mut fn_native = make_tx_sim(tx_hash, RunType::Native, &config)?;
    criterion.bench_function("sim_tx_native", |b| {
        b.iter(|| { fn_native() })
    });

    Ok(())
}

fn run_block_benchmarks(block_num: u64) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(20));

        
    criterion.bench_function("sim_block_aot_compiled", |b| {
        b.iter(|| run_block_sim(block_num, RunType::AOTCompiled))
    });

    criterion.bench_function("sim_block_native", |b| {
        b.iter(|| run_block_sim(block_num, RunType::Native))
    });

    Ok(())
}

fn run_call_benchmarks(call: Call, config: Config) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));

    let mut fn_jit = make_call_sim(call, RunType::JITCompiled, &config)?;
    criterion.bench_function("sim_call_jit", |b| {
        b.iter(|| { fn_jit() })
    });

    let mut fn_aot = make_call_sim(call, RunType::AOTCompiled, &config)?;
    criterion.bench_function("sim_call_aot_compiled", |b| {
        b.iter(|| { fn_aot() })
    });

    let mut fn_native = make_call_sim(call, RunType::Native, &config)?;
    criterion.bench_function("sim_call_native", |b| {
        b.iter(|| { fn_native() })
    });

    Ok(())
}

#[derive(Debug)]
pub enum RunType {
    Native,
    AOTCompiled,
    JITCompiled,
}

impl FromStr for RunType {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "native" => Ok(RunType::Native),
            "aot_compiled" => Ok(RunType::AOTCompiled),
            "jit_compiled" => Ok(RunType::JITCompiled),
            _ => Err(eyre::eyre!("Invalid run type")),
        }
    }
}

fn run_tx_sim(tx_hash: B256, run_type: RunType, config: Config) -> Result<()> {
    make_tx_sim(tx_hash, run_type, &config)?()
}

fn run_block_sim(block_num: u64, run_type: RunType) -> Result<()> {
    make_block_sim(block_num, run_type)?()
}

fn run_call_sim(call: Call, run_type: RunType, config: Config) -> Result<ExecutionResult> {
    make_call_sim(call, run_type, &config)?()
}

// todo: count contract touches + if it was external fn or not
// todo: get all contracts for the tx/block and compile them if missing (before bench and run) - cli flag
// todo: execute state till the tx
fn make_tx_sim(tx_hash: B256, run_type: RunType, config: &Config) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let Config { provider_factory, dir_path } = config;
    let (tx, meta) = provider_factory.
        transaction_by_hash_with_meta(tx_hash)?
        .ok_or_eyre("No tx found")?;
    let block = provider_factory
        .block(meta.block_number.into())?
        .ok_or_eyre("No block found")?;

    let env = env_with_handler_cfg(provider_factory.chain_spec().chain.id(), &block);
    let state_provider = Arc::new(provider_factory.history_by_block_number((meta.block_number-1).into())?);
    let db = CacheDB::new(StateProviderDatabase::new(state_provider.clone()));

    match run_type {
        RunType::Native => {
            let mut evm = revm::Evm::builder()
                .with_db(db)
                .with_env_with_handler_cfg(env)
                .build();
            return Ok(Box::new(move || {
                let res = sim::sim_txs(&vec![tx.clone()], &mut evm)?;
                // todo: check results are ok (eg. gas used)
                Ok(())
            }));
        },
        RunType::AOTCompiled => {
            let mut evm = utils::evm::create_evm(dir_path, db, Some(env), None)?;
            let tx = tx.clone();
            return Ok(Box::new(move || {
                let res = sim::sim_txs(&vec![tx.clone()], &mut evm)?;
                // todo: check results are ok (eg. gas used)
                // todo: check native touches are zero
                Ok(())
            }))
        }, 
        RunType::JITCompiled => {
            let path = default_build_config_path()?; // todo pass as arg
            let results = utils::build::compile_jit_from_file_path(Box::new(state_provider), &path)?
                .into_iter().collect::<Result<Vec<_>>>()?;
            let ext_ctx: revmc_sim_load::ExternalContext = results.into();
            let mut evm = revm::Evm::builder()
                .with_db(db)
                .with_external_context(ext_ctx)
                .append_handler_register(revmc_sim_load::register_handler)
                .build();
            let tx = tx.clone();
            return Ok(Box::new(move || {
                let res = sim::sim_txs(&vec![tx.clone()], &mut evm)?;
                // todo: check results are ok (eg. gas used)
                Ok(())
            }))
        }
    }
}

pub fn make_block_sim(block_num: u64, run_type: RunType) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let db_path = std::env::var("RETH_DB_PATH")?;
    let provider_factory = utils::evm::make_provider_factory(&db_path)?;

    let dir_path = std::env::current_dir()?.join(".data");
    let dir_path_str = dir_path.to_string_lossy().to_string();

    let block = provider_factory
        .block(block_num.into())?
        .ok_or_eyre("No block found")?;

    let env = env_with_handler_cfg(provider_factory.chain_spec().chain.id(), &block);
    let state_provider = provider_factory.history_by_block_number((block_num-1).into())?;
    let db = CacheDB::new(StateProviderDatabase::new(state_provider));

    match run_type {
        RunType::Native => {
            let mut evm = revm::Evm::builder()
                .with_db(db)
                .with_env_with_handler_cfg(env)
                .build();
            return Ok(Box::new(move || {
                let res = sim::sim_txs(&block.body, &mut evm)?;
                // todo: check results are ok (eg. gas used)
                Ok(())
            }))
        },
        RunType::AOTCompiled => {
            let mut evm = utils::evm::create_evm(&dir_path_str, db, Some(env), None)?;
            return Ok(Box::new(move || {
                let res = sim::sim_txs(&block.body, &mut evm)?;
                // todo: check results are ok (eg. gas used)
                Ok(())
            }))
        }, 
        RunType::JITCompiled => {
            todo!()
        }
    }
}

#[derive(Clone, Copy)]
enum Call {
    Fibbonacci
}

const FIBONACCI_CODE: &[u8] =
    &hex!("5f355f60015b8215601a578181019150909160019003916005565b9150505f5260205ff3");


fn make_call_sim(call: Call, run_type: RunType, config: &Config) -> Result<Box<dyn FnMut() -> Result<ExecutionResult>>> {
    let Config { provider_factory, dir_path } = config;
    let state_provider = Arc::new(provider_factory.latest()?);
    let mut db = CacheDB::new(StateProviderDatabase::new(state_provider.clone()));
    
    let tx_env = match call {
        Call::Fibbonacci => {
            let actual_num = U256::from(100_000);
            revmc_sim_build::compile_contract_aot(FIBONACCI_CODE, None)?;
            let bytecode = Bytecode::new_raw(FIBONACCI_CODE.into());
            let fibonacci_address = address!("0000000000000000000000000000000000001234");
            let mut account_info = AccountInfo::default();
            account_info.code_hash = bytecode.hash_slow();
            account_info.code = Some(bytecode);
            db.insert_account_info(fibonacci_address, account_info);
            let mut tx = TxEnv::default();
            tx.transact_to = TransactTo::Call(fibonacci_address);
            tx.data = actual_num.to_be_bytes_vec().into();
            tx
        }
    };
    make_call_fn(tx_env, run_type, db, state_provider, Some(dir_path))
}

fn make_call_fn<ExtDB: revm::Database + revm::DatabaseRef + 'static>(
    tx: TxEnv,
    run_type: RunType,
    db: CacheDB<ExtDB>,
    state_provider: Arc<impl StateProvider + ?Sized>,
    dir_path: Option<&str>, // todo: make this related to enum type instead
) -> Result<Box<dyn FnMut() -> Result<ExecutionResult>>> 
where <ExtDB as revm::DatabaseRef>::Error: std::fmt::Debug
{
    match run_type {
        RunType::Native => {
            let mut evm = revm::Evm::builder()
                .with_db(db)
                .build();
            return Ok(Box::new(move || {
                evm.context.evm.env.tx = tx.clone();
                let result = evm.transact().unwrap();
                Ok(result.result)
            }));
        },
        RunType::AOTCompiled => {
            let dir_path = dir_path.ok_or_else(|| eyre::eyre!("Missing dir path"))?;
            let mut evm = utils::evm::create_evm(dir_path, db, None, None)?;
            return Ok(Box::new(move || {
                evm.context.evm.env.tx = tx.clone();
                let result = evm.transact().unwrap();
                Ok(result.result)
            }))
        }, 
        RunType::JITCompiled => {
            let path = default_build_config_path()?; // todo pass as arg
            let results = utils::build::compile_jit_from_file_path(Box::new(state_provider), &path)?
                .into_iter().collect::<Result<Vec<_>>>()?;
            let ext_ctx: revmc_sim_load::ExternalContext = results.into();
            let mut evm = revm::Evm::builder()
                .with_db(db)
                .with_external_context(ext_ctx)
                .append_handler_register(revmc_sim_load::register_handler)
                .build();
            return Ok(Box::new(move || {
                evm.context.evm.env.tx = tx.clone();
                let result = evm.transact().unwrap();
                Ok(result.result)
            }))
        }
    }
}

fn env_with_handler_cfg(chain_id: u64, block: &Block) -> EnvWithHandlerCfg {
    let block_env = block_env_from_block(block);
    let cfg = CfgEnv::default().with_chain_id(chain_id);
    let cfg_env = CfgEnvWithHandlerCfg::new_with_spec_id(cfg, SpecId::CANCUN);
    let env = EnvWithHandlerCfg::new_with_cfg_env(cfg_env, block_env, TxEnv::default());
    env
}

// todo: do this myself or find a better way
fn block_env_from_block(block: &Block) -> BlockEnv {
    let mut block_env = BlockEnv::default();
    let eth_evm_cfg = EthEvmConfig::default();
    eth_evm_cfg.fill_block_env(
        &mut block_env,
        &block.header,
        block.header.number >= 15537393,
    );
    block_env
}