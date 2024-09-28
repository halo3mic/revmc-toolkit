use std::sync::Arc;
use eyre::{Result, OptionExt};
use revm::{
    db::{CacheDB, InMemoryDB}, 
    handler::register::HandleRegister, primitives::{
        AccountInfo, Address, Bytecode, Bytes, EnvWithHandlerCfg, TransactTo, TxEnv, B256
    }, Database, DatabaseRef, Evm
};
use reth_provider::{
    StateProvider, ProviderFactory, BlockReader, 
    ChainSpecProvider, TransactionsProvider,
};
use reth_primitives::{Block, TransactionSigned, TransactionMeta};
use reth_revm::database::StateProviderDatabase;
use reth_rpc_types::mev::EthCallBundleResponse;
use reth_db::DatabaseEnv;


pub type StateProviderCacheDB = CacheDB<StateProviderDatabase<StateProviderArc>>;
type SimEvm<'a, ExtCtx> = Evm<'a, ExtCtx, StateProviderCacheDB>;
type SimFn<'a, ExtCtx, DB> = Box<dyn FnMut(&mut Evm<'a, ExtCtx, DB>) -> Result<Vec<SimResult>>>;
type StateProviderArc = Arc<Box<dyn StateProvider>>;
type SimResults = Vec<SimResult>;

use crate::tx_sim;
use crate::utils;


pub trait CallSimBuilderExt<ExtCtx> {
    fn into_call_sim(self, bytecode: Bytecode, input: Bytes) -> Result<Simulation<ExtCtx, InMemoryDB>>;
}

impl<ExtCtx: 'static, P> CallSimBuilderExt<ExtCtx> for SimulationBuilder<ExtCtx, P> {
    fn into_call_sim(mut self, bytecode: Bytecode, input: Bytes) -> Result<Simulation<ExtCtx, InMemoryDB>> {
        let fibonacci_address = Address::from_slice(&[1; 20]);
        let mut account_info = AccountInfo::default();
        account_info.code_hash = bytecode.hash_slow();
        account_info.code = Some(bytecode);

        let mut db = InMemoryDB::default();
        db.insert_account_info(fibonacci_address, account_info);

        let mut tx_env = TxEnv::default();
        tx_env.transact_to = TransactTo::Call(fibonacci_address);
        tx_env.data = input;

        let ext_ctx = self.ext_ctx.take().ok_or_eyre("No external context")?;
        let evm = utils::evm::make_evm(db, ext_ctx, None, None);

        let execute_fn = Box::new(move |evm: &mut Evm<ExtCtx, InMemoryDB>| {
            evm.context.evm.env.tx = tx_env.clone();
            let result = evm.transact()?;
            Ok(vec![result.result.into()])
        });

        Ok(Simulation::new(evm, execute_fn))
    }
}


pub trait TxsSimBuilderExt<ExtCtx, S> {
    type SimType = S;

    fn provider_factory(&self) -> &Arc<ProviderFactory<DatabaseEnv>>;

    fn into_tx_sim(
        self, 
        tx_hash: B256
    ) -> Result<S>;
    fn into_block_sim(
        self, 
        block_number: u64, 
        block_chunk: Option<BlockPart>
    ) -> Result<S>;
    fn make_txs_sim(
        self, 
        block: &Block,
        tx_hashes: Vec<TransactionSigned>, // todo could just take incidces instaed
        pre_execution_txs: Vec<TransactionSigned>,
    ) -> Result<S>;

    fn make_sim<PF, InnerDB: Database + DatabaseRef + Clone>(
        mut evm: Evm<'static, ExtCtx, CacheDB<InnerDB>>, 
        execute_fn: SimFn<'static, ExtCtx, CacheDB<InnerDB>>, 
        preexecute_fn: Option<Box<PF>>,
    ) -> Result<Simulation<ExtCtx, CacheDB<InnerDB>>> 
    where 
        PF: FnOnce(&mut Evm<'static, ExtCtx, CacheDB<InnerDB>>) -> Result<SimResults>,
    {
        let pre_res = preexecute_fn.map(|f| f(&mut evm))
            .transpose()?
            .unwrap_or_default();
        Ok(Simulation::new(evm, execute_fn).with_pre_execution_res(pre_res))
    }

    fn make_db_at_block(&self, block_number: u64) -> Result<StateProviderCacheDB> {
        let state_provider = Arc::new(self.provider_factory().history_by_block_number(block_number-1)?);
        Ok(CacheDB::new(StateProviderDatabase::new(state_provider)))
    }

    fn make_env(&self, block: &Block) -> EnvWithHandlerCfg {
        let chain_id = self.provider_factory().chain_spec().chain.id();
        utils::evm::env_with_handler_cfg(chain_id, block)
    }

    fn get_block(&self, block_number: u64) -> Result<Block> {
        self.provider_factory().block(block_number.into())?
            .ok_or_eyre("No block found")
    }

    fn get_tx_with_meta(&self, tx_hash: B256) -> Result<(TransactionSigned, TransactionMeta)> {
        self.provider_factory().transaction_by_hash_with_meta(tx_hash)?
            .ok_or_eyre("No tx found")
    }
}

impl<ExtCtx: 'static> TxsSimBuilderExt<ExtCtx, Simulation<ExtCtx, StateProviderCacheDB>> 
for SimulationBuilder<ExtCtx, Arc<ProviderFactory<DatabaseEnv>>> 
{
    
    fn provider_factory(&self) -> &Arc<ProviderFactory<DatabaseEnv>> {
        &self.provider_factory
    }
    
    fn into_tx_sim(self, tx_hash: B256) -> Result<Self::SimType> {
        let (tx, meta) = self.get_tx_with_meta(tx_hash)?;
        let block = self.get_block(meta.block_number.into())?;
        let pre_execution_txs = block.body[..(meta.index as usize)].to_vec();
        self.make_txs_sim(&block, vec![tx], pre_execution_txs)
    }

    fn into_block_sim(
        self, 
        block_number: u64, 
        block_chunk: Option<BlockPart>
    ) -> Result<Self::SimType> {
        let block = self.get_block(block_number)?;
        let txs = block.body.clone();
        let (txs, pre_execution) = match block_chunk {
            Some(chunk) => chunk.split_txs(txs),
            None => (txs, vec![]),
        };
        self.make_txs_sim(&block, txs, pre_execution)
    }

    fn make_txs_sim(
        mut self, 
        block: &Block,
        tx_hashes: Vec<TransactionSigned>, // todo could just take incidces instaed
        pre_execution_txs: Vec<TransactionSigned>,
    ) -> Result<Self::SimType> {
        let db = self.make_db_at_block(block.number)?;
        let env = self.make_env(block);
        let evm = utils::evm::make_evm(
            db, 
            self.ext_ctx.take()
                .ok_or_eyre("No external context")?, 
            self.handler_register.take(), 
            Some(env)
        );

        let execute_fn = Box::new(move |evm: &mut SimEvm<ExtCtx>| {
            tx_sim::sim_txs(&tx_hashes.clone(), evm)
                .map(|r| r.into_sim_results())
        });
        let preexecute_fn = Box::new(|evm: &mut SimEvm<ExtCtx>| {
            tx_sim::sim_txs(&pre_execution_txs, evm)
                .map(|r| r.into_sim_results())
        });

        Self::make_sim(evm, execute_fn, Some(preexecute_fn))
    }
}


pub struct SimulationBuilder<ExtCtx, P> {
    provider_factory: P,
    ext_ctx: Option<ExtCtx>,
    handler_register: Option<HandleRegister<ExtCtx, StateProviderCacheDB>>,
}

impl Default for SimulationBuilder<(), ()> {
    fn default() -> Self {
        Self {
            provider_factory: (),
            ext_ctx: None,
            handler_register: None,
        }
    }
}

impl<ExtCtx, P> SimulationBuilder<ExtCtx, P> {

    pub fn with_ext_ctx<'a, ExtCtxInner>(self, ext_ctx: ExtCtxInner) -> SimulationBuilder<ExtCtxInner, P> {
        SimulationBuilder { 
            provider_factory: self.provider_factory,
            handler_register: None,
            ext_ctx: Some(ext_ctx),
        }
    }

    pub fn with_handle_register(self, handle_register: HandleRegister<ExtCtx, StateProviderCacheDB>) -> Self {
        Self {
            provider_factory: self.provider_factory,
            handler_register: Some(handle_register),
            ext_ctx: self.ext_ctx,
        }
    }

}

impl<ExtCtx> SimulationBuilder<ExtCtx, ()> {

    pub fn with_provider_factory(
        self, 
        provider_factory: Arc<ProviderFactory<DatabaseEnv>>
    ) -> SimulationBuilder<ExtCtx, Arc<ProviderFactory<DatabaseEnv>>> {
        SimulationBuilder {
            ext_ctx: None,
            handler_register: None,
            provider_factory,
        }
    }

}

pub struct Simulation<ExtCtx: 'static, DB: Database + DatabaseRef + 'static> {
    evm: Evm<'static, ExtCtx, DB>,
    fnc: Box<dyn FnMut(&mut Evm<'static, ExtCtx, DB>) -> Result<Vec<SimResult>>>,
    pre_execution_res: Option<Vec<SimResult>>,
}

impl<ExtCtx, DB> Simulation<ExtCtx, DB> 
    where DB: Database + DatabaseRef + Clone,
{

    pub fn new(evm: Evm<'static, ExtCtx, DB>, fnc: SimFn<'static, ExtCtx, DB>) -> Self {
        Self { evm, fnc, pre_execution_res: None, }
    }

    pub fn run(&mut self) -> Result<Vec<SimResult>> {
        let prev_db = self.evm.db().clone();
        let res = (self.fnc)(&mut self.evm)?;
        *self.evm.db_mut() = prev_db;
        Ok(res)
    }

    pub fn into_evm(self) -> Evm<'static, ExtCtx, DB> {
        self.evm
    }

    pub fn evm(&self) -> &Evm<'static, ExtCtx, DB> {
        &self.evm
    }

    fn with_pre_execution_res(mut self, pre_execution_res: Vec<SimResult>) -> Self {
        self.pre_execution_res = Some(pre_execution_res);
        self
    }

    pub fn pre_execution_res(&self) -> Option<&Vec<SimResult>> {
        self.pre_execution_res.as_ref()
    }

}

#[derive(Clone, Copy, Debug)]
pub enum BlockPart {
    TOB(f32),
    BOB(f32)
}

impl BlockPart {

    fn split_txs<T>(&self, mut txs: Vec<T>) -> (Vec<T>, Vec<T>) {
        let mut pre_execution = vec![];
        match self {
            BlockPart::TOB(chunk) => {
                let chunk_size = (txs.len() as f32 * chunk).ceil() as usize;
                txs = txs.into_iter().take(chunk_size).collect();
            },
            BlockPart::BOB(chunk) => {
                let chunk_size = (txs.len() as f32 * chunk).ceil() as usize;
                pre_execution = txs.drain(..chunk_size).collect();
            }
        }
        (txs, pre_execution)
    }

}

#[derive(Default, Debug)]
pub struct SimResult {
    pub gas_used: u64, 
    pub success: bool,
    pub output: Option<Bytes>,
}

impl SimResult {
    fn with_gas_used(mut self, gas_used: u64) -> Self {
        self.gas_used = gas_used;
        self
    }
    fn with_success(mut self, success: bool) -> Self {
        self.success = success;
        self
    }
    fn with_output(mut self, output: Bytes) -> Self {
        self.output = Some(output);
        self
    }
}

pub trait IntoSimResults {
    fn into_sim_results(self) -> Vec<SimResult>;
}

impl IntoSimResults for EthCallBundleResponse {
    fn into_sim_results(self) -> Vec<SimResult> {
        self.results
            .into_iter()
            .map(|r| {
                SimResult::default()
                    .with_gas_used(r.gas_used)
                    .with_success(r.revert.is_none())
                })
            .collect()
    }
}

impl From<revm::primitives::ExecutionResult> for SimResult {
    fn from(res: revm::primitives::ExecutionResult) -> Self {
        let sim_res = Self::default()
            .with_gas_used(res.gas_used())
            .with_success(res.is_success());
        if let Some(output) = res.output() {
            sim_res.with_output(output.clone())
        } else {
            sim_res
        }
    }
}