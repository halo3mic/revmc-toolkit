use revm::{
    primitives::{
        EnvWithHandlerCfg, TxEnv, BlockEnv, CfgEnvWithHandlerCfg, SpecId, 
        CfgEnv, B256, Address, Bytes, AccountInfo, Bytecode, TransactTo, 
        address, U256,
    },
    db::CacheDB,
};
use reth_provider::{
    ReceiptProvider, StateProvider, ProviderFactory, BlockReader, 
    ChainSpecProvider, TransactionsProvider,
};
use reth_primitives::{Block, TransactionSigned};
use reth_revm::database::StateProviderDatabase;
use reth_db::DatabaseEnv;
use revmc::primitives::{hex, keccak256};

use std::{str::FromStr, sync::Arc};
use eyre::{Result, OptionExt};
use tracing::debug;


use crate::utils;

pub struct SimConfig {
    pub provider_factory: Arc<ProviderFactory<DatabaseEnv>>,
    pub dir_path: String,
}

impl SimConfig {
    pub fn new(provider_factory: Arc<ProviderFactory<DatabaseEnv>>, dir_path: String) -> Self {
        Self { provider_factory, dir_path }
    }
}

#[derive(Debug)]
pub enum SimRunType {
    Native,
    AOTCompiled,
    JITCompiled,
}

impl FromStr for SimRunType {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "native" => Ok(SimRunType::Native),
            "aot_compiled" => Ok(SimRunType::AOTCompiled),
            "jit_compiled" => Ok(SimRunType::JITCompiled),
            _ => Err(eyre::eyre!("Invalid run type")),
        }
    }
}

pub fn run_tx_sim(tx_hash: B256, run_type: SimRunType, config: &SimConfig) -> Result<()> {
    make_tx_sim(tx_hash, run_type, config)?()
}

pub fn run_block_sim(block_num: u64, run_type: SimRunType, config: &SimConfig) -> Result<()> {
    make_block_sim(block_num, run_type, config)?()
}

pub fn run_call_sim(call: SimCall, run_type: SimRunType, config: &SimConfig) -> Result<()> {
    make_call_sim(call, run_type, config)?()
}

pub fn make_tx_sim(tx_hash: B256, run_type: SimRunType, config: &SimConfig) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let SimConfig { provider_factory, dir_path } = config;

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

pub fn make_block_sim(block_num: u64, run_type: SimRunType, config: &SimConfig) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let SimConfig { provider_factory, dir_path } = config;

    let block = provider_factory
        .block(block_num.into())?
        .ok_or_eyre("No block found")?;
    let txs = Arc::new(block.body.clone());

    make_txs_sim(txs, run_type, &provider_factory, &dir_path, &block, block.header.gas_used, vec![])
}

#[derive(Clone, Copy, Debug)]
pub enum SimCall {
    Fibbonacci
}

const FIBONACCI_CODE: &[u8] =
    &hex!("5f355f60015b8215601a578181019150909160019003916005565b9150505f5260205ff3");


pub fn make_call_sim(call: SimCall, run_type: SimRunType, config: &SimConfig) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let SimConfig { provider_factory, dir_path } = config;
    let state_provider = Arc::new(provider_factory.latest()?);
    let mut db = CacheDB::new(StateProviderDatabase::new(state_provider.clone()));
    
    let (tx_env, exepected_gas, expected_out) = match call {
        SimCall::Fibbonacci => {
            let actual_num = U256::from(100_000);
            let expected_target_gas = 6_321_215;
            let expected_result = Bytes::from_str("0xf77c8c850c19775591850bc3769fd422f84fdf260a20dd8ac7ee006d287ebc5d")?;
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
            (tx, expected_target_gas, expected_result)
        }
    };
    let execute_fn = move |evm: &mut EvmWithExtCtx| {
        evm.context.evm.env.tx = tx_env.clone();
        let result = evm.transact()?;

        // ! This will incur some conditional latency
        if let Some(actual_out) = result.result.output() {
            if actual_out != &expected_out {
                return Err(eyre::eyre!("Output mismatch; expected {expected_out} got {actual_out}"));
            }
        }
        Ok(result.result.into())
    };

    let (evm, all_non_native) = prepare_evm_for_runtype(
        run_type, 
        state_provider,
        db,
        &dir_path, 
        execute_fn.clone(), 
        None,
    )?;
    make_sim(
        evm, 
        execute_fn, 
        None::<Box<fn(&mut EvmWithExtCtx) -> Result<MyExecutionResult>>>, 
        exepected_gas, 
        all_non_native
    )
}

type EvmWithExtCtx<'a> = revm::Evm<'a, revmc_sim_load::ExternalContext, CacheDB<StateProviderDatabase<Arc<Box<dyn StateProvider>>>>>;

fn make_txs_sim(
    txs: Arc<Vec<TransactionSigned>>, 
    run_type: SimRunType, 
    provider_factory: &Arc<ProviderFactory<DatabaseEnv>>, 
    dir_path: &str, // todo include this in RunType enum
    block: &Block, 
    exepected_gas_used: u64, 
    pre_execution_txs: Vec<TransactionSigned>,
) -> Result<Box<dyn FnMut() -> Result<()>>> {
    let state_provider = Arc::new(provider_factory.history_by_block_number((block.number-1).into())?);
    let db = CacheDB::new(StateProviderDatabase::new(state_provider.clone()));
    let env = env_with_handler_cfg(provider_factory.chain_spec().chain.id(), &block);

    let execute_fn = move |evm: &mut EvmWithExtCtx| {
        utils::sim::sim_txs(&txs.clone(), evm).map(|r| r.into())
    };
    let preexecute_fn = Box::new(|evm: &mut EvmWithExtCtx| {
        utils::sim::sim_txs(&pre_execution_txs, evm).map(|r| r.into())
    });
    let (evm, all_non_native) = prepare_evm_for_runtype(
        run_type,
        state_provider,
        db, 
        dir_path, 
        execute_fn.clone(), 
        Some(env),
    )?;
    make_sim(evm, execute_fn, Some(preexecute_fn), exepected_gas_used, all_non_native)
}

fn prepare_evm_for_runtype(
    run_type: SimRunType,
    state_provider: Arc<Box<dyn StateProvider>>,
    db: CacheDB<StateProviderDatabase<Arc<Box<dyn StateProvider>>>>,
    dir_path: &str, 
    execute_fn: impl Fn(&mut EvmWithExtCtx) -> Result<MyExecutionResult> + 'static,
    env: Option<EnvWithHandlerCfg>,
) -> Result<(EvmWithExtCtx<'static>, bool)> {
    let (evm, all_non_native) = match run_type {
        SimRunType::Native => (make_evm(db, None, env), false),
        SimRunType::AOTCompiled => {
            let fnc = move |evm: &mut EvmWithExtCtx| execute_fn(evm).map(|_| ());
            let selected = aot_compile_touched_contracts(state_provider.clone(), db.clone(), env.clone(), fnc)?;
            let ext_ctx = revmc_sim_load::build_external_context(&dir_path, Some(selected))?;
            (make_evm(db, Some(ext_ctx), env), true)
        }, 
        SimRunType::JITCompiled => {
            // todo: compile touched contracts

            let path = utils::default_build_config_path()?; // todo pass as arg
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
        // let expected_target_gas = expected_target_gas - pre_res.gas_used;
        // let actual_gas = res.gas_used;
        // if actual_gas != expected_target_gas {
        //     return Err(eyre::eyre!("Gas used mismatch; expected {expected_target_gas} got {actual_gas}"));
        // }
        // todo: the bottom part will inccur some conditional latency
        debug!("Touches: {:?}", evm.context.external.touches);
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

use reth_evm_ethereum::EthEvmConfig;
use reth_evm::ConfigureEvmEnv;

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
    state_provider: Arc<Box<dyn StateProvider>>,
    db: CacheDB<ExtDB>,
    env: Option<EnvWithHandlerCfg>,
    run_fn: F
) -> Result<Vec<B256>> 
where 
    F: FnOnce(&mut revm::Evm<revmc_sim_load::ExternalContext, CacheDB<ExtDB>>) -> Result<()>,
    <ExtDB as revm::DatabaseRef>::Error: std::error::Error + Send + Sync + 'static,
    ExtDB: Clone,
{
    let mut evm = revm::Evm::builder()
        .with_db(db.clone())
        .with_external_context(revmc_sim_load::ExternalContext::default())
        .with_env_with_handler_cfg(env.unwrap_or_default())
        .append_handler_register(revmc_sim_load::register_handler)
        .build();

    run_fn(&mut evm)?;
    let touched_contracts = evm.context.external.touches
        .expect("Expected at least one touch")
        .into_iter()
        .inspect(|(address, counter)| {
            debug!("Touched contract: {address:?} {counter:?}");
        })
        .map(|(address, _counter)| address)
        .collect::<Vec<Address>>();

    let contracts = touched_contracts.iter()
        .map(|account| {
            match db.accounts.get(account) {
                Some(account) => {
                    let code = account.info.code.as_ref()
                        .ok_or_eyre("No code found")?;
                    Ok(code.original_byte_slice().to_vec())
                },
                None => {
                    let code = state_provider.account_code(*account)?
                        .ok_or_eyre("No code found for address")?;
                    Ok(code.original_byte_slice().to_vec())
                }
            }
        })
        .collect::<Result<Vec<_>>>()?;
    
    let code_hashes = contracts.iter().map(|c| keccak256(c)).collect();
    utils::build::compile_aot_from_codes(contracts, None)?;
    

    Ok(code_hashes)
}
