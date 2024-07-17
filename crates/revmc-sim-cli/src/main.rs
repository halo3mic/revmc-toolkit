mod utils;
mod sim;
mod cli;

use reth_provider::{ReceiptProvider, StateProvider};
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
use reth_primitives::{Block, TransactionSigned};
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
    let config = Config { dir_path, provider_factory };

    let cli = Cli::parse();
    match cli.command {
        Commands::Build(_) => {
            // todo: parse config path
            let state_provider = config.provider_factory.latest()?;
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
                run_tx_sim(tx_hash, run_type, &config)?;
            } else if let Some(block_num) = run_args.block_num {
                let block_num = block_num.parse::<u64>()?;
                println!("Running sim for block: {block_num:?}");
                run_block_sim(block_num, run_type, &config)?;
            } else {
                // todo: format
                run_call_sim(Call::Fibbonacci, run_type, &config)?;
                return Err(eyre::eyre!("Please provide either a transaction hash or a block number."));
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
                run_call_benchmarks(Call::Fibbonacci, &config)?;
            }
        }
    }
    
    Ok(())

}

fn run_tx_benchmarks(tx_hash: B256, config: &Config) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));

    let mut fn_jit = make_tx_sim(tx_hash, RunType::JITCompiled, config)?;
    criterion.bench_function("sim_tx_jit_compiled", |b| {
        b.iter(|| { fn_jit() })
    });

    let mut fn_aot = make_tx_sim(tx_hash, RunType::AOTCompiled, config)?;
    criterion.bench_function("sim_tx_aot_compiled", |b| {
        b.iter(|| { fn_aot() })
    });

    let mut fn_native = make_tx_sim(tx_hash, RunType::Native, config)?;
    criterion.bench_function("sim_tx_native", |b| {
        b.iter(|| { fn_native() })
    });

    Ok(())
}

fn run_block_benchmarks(block_num: u64, config: &Config) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .measurement_time(Duration::from_secs(20));

    // todo jit

    criterion.bench_function("sim_block_aot_compiled", |b| {
        b.iter(|| run_block_sim(block_num, RunType::AOTCompiled, config))
    });

    criterion.bench_function("sim_block_native", |b| {
        b.iter(|| run_block_sim(block_num, RunType::Native, config))
    });

    Ok(())
}

fn run_call_benchmarks(call: Call, config: &Config) -> Result<()> {
    let mut criterion = Criterion::default()
        .sample_size(200)
        .measurement_time(Duration::from_secs(30));

    let mut fn_jit = make_call_sim(call, RunType::JITCompiled, config)?;
    criterion.bench_function("sim_call_jit", |b| {
        b.iter(|| { fn_jit() })
    });

    let mut fn_aot = make_call_sim(call, RunType::AOTCompiled, config)?;
    criterion.bench_function("sim_call_aot_compiled", |b| {
        b.iter(|| { fn_aot() })
    });

    let mut fn_native = make_call_sim(call, RunType::Native, config)?;
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

fn run_tx_sim(tx_hash: B256, run_type: RunType, config: &Config) -> Result<()> {
    make_tx_sim(tx_hash, run_type, config)?()
}

fn run_block_sim(block_num: u64, run_type: RunType, config: &Config) -> Result<()> {
    make_block_sim(block_num, run_type, config)?()
}

fn run_call_sim(call: Call, run_type: RunType, config: &Config) -> Result<()> {
    make_call_sim(call, run_type, config)?()
}

fn make_tx_sim(tx_hash: B256, run_type: RunType, config: &Config) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let Config { provider_factory, dir_path } = config;

    let (tx, meta) = provider_factory.
        transaction_by_hash_with_meta(tx_hash)?
        .ok_or_eyre("No tx found")?;
    let block = provider_factory
        .block(meta.block_number.into())?
        .ok_or_eyre("No block found")?;
    let pre_execution_txs = block.body[..(meta.index as usize)].to_vec(); // todo: what if tx idx is zero
    let exepected_gas_used = provider_factory.receipt_by_hash(tx_hash)?
        .map(|receipt| receipt.cumulative_gas_used)
        .unwrap_or_default();
    let txs = Arc::new(vec![tx]);

    make_txs_sim(txs, run_type, provider_factory, &dir_path, &block, exepected_gas_used, pre_execution_txs)
}

fn make_block_sim(block_num: u64, run_type: RunType, config: &Config) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let Config { provider_factory, dir_path } = config;

    let block = provider_factory
        .block(block_num.into())?
        .ok_or_eyre("No block found")?;
    let txs = Arc::new(block.body.clone());

    make_txs_sim(txs, run_type, &provider_factory, &dir_path, &block, block.header.gas_used, vec![])
}

#[derive(Clone, Copy)]
enum Call {
    Fibbonacci
}

const FIBONACCI_CODE: &[u8] =
    &hex!("5f355f60015b8215601a578181019150909160019003916005565b9150505f5260205ff3");


fn make_call_sim(call: Call, run_type: RunType, config: &Config) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let Config { provider_factory, dir_path } = config;
    let state_provider = Arc::new(provider_factory.latest()?);
    let mut db = CacheDB::new(StateProviderDatabase::new(state_provider.clone()));
    
    let (tx_env, exepected_target_gas) = match call {
        Call::Fibbonacci => {
            let exepected_target_gas = 1_000_000;
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
            (tx, exepected_target_gas)
        }
    };
    let execute_fn = move |evm: &mut EvmWithExtCtx| {
        evm.context.evm.env.tx = tx_env.clone();
        let result = evm.transact()?;
        println!("Result: {:?}", result);
        Ok(result.result.into())
    };

    let (evm, all_non_native) = prepare_evm_for_runtype(
        run_type, 
        &provider_factory, 
        &dir_path, 
        &Block::default(), 
        execute_fn.clone(), 
        false,
    )?;
    make_sim(
        evm, 
        execute_fn, 
        None::<Box<fn(&mut EvmWithExtCtx) -> Result<MyExecutionResult>>>, 
        exepected_target_gas, 
        all_non_native
    )
}

type EvmWithExtCtx<'a> = revm::Evm<'a, revmc_sim_load::ExternalContext, CacheDB<StateProviderDatabase<Arc<Box<dyn StateProvider>>>>>;

fn make_txs_sim(
    txs: Arc<Vec<TransactionSigned>>, 
    run_type: RunType, 
    provider_factory: &Arc<ProviderFactory<DatabaseEnv>>, 
    dir_path: &str, // todo include this in RunType enum
    block: &Block, 
    exepected_gas_used: u64, 
    pre_execution_txs: Vec<TransactionSigned>,
) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let execute_fn = move |evm: &mut EvmWithExtCtx| {
        sim::sim_txs(&txs.clone(), evm).map(|r| r.into())
    };
    let preexecute_fn = Box::new(|evm: &mut EvmWithExtCtx| {
        sim::sim_txs(&pre_execution_txs, evm).map(|r| r.into())
    });
    let (evm, all_non_native) = prepare_evm_for_runtype(run_type, provider_factory, dir_path, block, execute_fn.clone(), true)?;
    make_sim(evm, execute_fn, Some(preexecute_fn), exepected_gas_used, all_non_native)
}

fn prepare_evm_for_runtype(
    run_type: RunType, 
    provider_factory: &Arc<ProviderFactory<DatabaseEnv>>, 
    dir_path: &str, 
    block: &Block,
    execute_fn: impl Fn(&mut EvmWithExtCtx) -> Result<MyExecutionResult> + 'static,
    with_env: bool,
) -> Result<(EvmWithExtCtx<'static>, bool)> {
    let env = 
        if with_env {
            Some(env_with_handler_cfg(provider_factory.chain_spec().chain.id(), &block))
        } else {
            None
        };
    let state_provider = Arc::new(provider_factory.history_by_block_number((block.number-1).into())?);
    let db = CacheDB::new(StateProviderDatabase::new(state_provider.clone()));

    let (evm, all_non_native) = match run_type {
        RunType::Native => (make_evm(db, None, env), false),
        RunType::AOTCompiled => {
            let fnc = move |evm: &mut EvmWithExtCtx| execute_fn(evm).map(|_| ());
            aot_compile_touched_contracts(db.clone(), &state_provider, env.clone(), fnc)?;// todo avoid cloning
            let ext_ctx = revmc_sim_load::build_external_context(&dir_path, None)?;
            (make_evm(db, Some(ext_ctx), env), true)
        }, 
        RunType::JITCompiled => {
            let path = default_build_config_path()?; // todo pass as arg
            let ext_fns = utils::build::compile_jit_from_file_path(Box::new(state_provider), &path)?
                .into_iter().collect::<Result<Vec<_>>>()?;
            (make_evm(db, Some(ext_fns.into()), env), true)
        }
    };
    Ok((evm, all_non_native))
}

fn make_sim<F, PF>(
    mut evm: EvmWithExtCtx<'static>, 
    execute_fn: F, 
    preexecute_fn: Option<Box<PF>>,
    expected_target_gas: u64,
    all_non_native: bool,
) -> Result<Box<dyn FnMut() -> Result<()>>> 
where 
    F: Fn(&mut EvmWithExtCtx) -> Result<MyExecutionResult> + 'static,
    PF: FnOnce(&mut EvmWithExtCtx) -> Result<MyExecutionResult>,
{
    let pre_res = preexecute_fn.map(|f| f(&mut evm)).transpose()?.unwrap_or_default();
    return Ok(Box::new(move || {
        let res = execute_fn(&mut evm)?;
        let expected_target_gas = expected_target_gas - pre_res.gas_used;
        let actual_gas = res.gas_used;
        if actual_gas != expected_target_gas {
            return Err(eyre::eyre!("Gas used mismatch; expected {expected_target_gas} got {actual_gas}"));
        }
        // todo: the bottom part will inccur some conditional latency
        if all_non_native {
            if let Some(touches) = &evm.context.external.touches {
                let frst_native_touch = touches.iter().find(|(_, c)| c.non_native > 0);
                if let Some(native_touch) = frst_native_touch {
                    return Err(eyre::eyre!("Expected no native touches; found for account {:?}", native_touch.0));
                }
            }
        }
        Ok(())
    }));
}

#[derive(Default)]
struct MyExecutionResult {
    gas_used: u64, 
    success: bool,
}

impl From<reth_rpc_types::EthCallBundleResponse> for MyExecutionResult {
    fn from(res: reth_rpc_types::EthCallBundleResponse) -> Self {
        Self {
            gas_used: res.total_gas_used,
            success: res.results.iter().all(|r| r.revert.is_none()),
        }
    }
}

impl From<revm::primitives::ExecutionResult> for MyExecutionResult {
    fn from(res: revm::primitives::ExecutionResult) -> Self {
        Self {
            gas_used: res.gas_used(),
            success: res.is_success(),
        }
    }
}


fn make_evm<'a>(
    db: CacheDB<StateProviderDatabase<Arc<Box<dyn StateProvider>>>>, 
    ext_ctx: Option<revmc_sim_load::ExternalContext>,
    env: Option<EnvWithHandlerCfg>,
) -> revm::Evm<'a, revmc_sim_load::ExternalContext, CacheDB<StateProviderDatabase<Arc<Box<dyn StateProvider>>>>> {
    revm::Evm::builder()
        .with_db(db)
        .with_external_context(ext_ctx.unwrap_or_default())
        .with_env_with_handler_cfg(env.unwrap_or_default())
        .append_handler_register(revmc_sim_load::register_handler)
        .build()
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

fn aot_compile_touched_contracts<ExtDB: revm::Database + revm::DatabaseRef, F>(
    db: CacheDB<ExtDB>,
    state_provider: &Box<impl StateProvider + ?Sized>,
    env: Option<EnvWithHandlerCfg>,
    run_fn: F
) -> Result<()> 
where 
    F: FnOnce(&mut revm::Evm<revmc_sim_load::ExternalContext, CacheDB<ExtDB>>) -> Result<()>,
    <ExtDB as revm::DatabaseRef>::Error: std::fmt::Debug
{
    let mut evm = revm::Evm::builder()
        .with_db(db)
        .with_external_context(revmc_sim_load::ExternalContext::default())
        .with_env_with_handler_cfg(env.unwrap_or_default())
        .append_handler_register(revmc_sim_load::register_handler)
        .build();

    run_fn(&mut evm)?;
    let touched_contracts = evm.context.external.touches
        .expect("Expected at least one touch")
        .into_iter()
        .map(|(address, _counter)| address)
        .collect::<Vec<Address>>();

    utils::build::compile_aot_from_contracts(state_provider, &touched_contracts, None)? // todo: pass options
        .into_iter().collect::<Result<Vec<_>>>()?;

    Ok(())
}
