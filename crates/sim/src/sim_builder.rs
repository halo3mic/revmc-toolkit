use eyre::{OptionExt, Result};
use reth_db::DatabaseEnv;
use reth_primitives::{Block, Header, TransactionMeta, TransactionSigned};
use reth_provider::{
    BlockReader, ChainSpecProvider, ProviderFactory, StateProvider, TransactionsProvider,
};
use reth_revm::database::StateProviderDatabase;
use reth_rpc_types::mev::EthCallBundleResponse;
use revm::{
    db::{CacheDB, InMemoryDB},
    handler::register::HandleRegister,
    primitives::{
        AccountInfo, Address, Bytecode, Bytes, EnvWithHandlerCfg, TransactTo, TxEnv, B256,
    },
    Database, DatabaseRef, Evm,
};
use std::sync::Arc;

pub type StateProviderCacheDB = CacheDB<StateProviderDatabase<StateProviderArc>>;
type SimEvm<'a, ExtCtx> = Evm<'a, ExtCtx, StateProviderCacheDB>;
type SimFn<'a, ExtCtx, DB> = Box<dyn FnMut(&mut Evm<'a, ExtCtx, DB>) -> Result<Vec<SimResult>>>;
type StateProviderArc = Arc<Box<dyn StateProvider>>;
type SimResults = Vec<SimResult>;

use crate::tx_sim;
use crate::utils;

pub trait CallSimBuilderExt<ExtCtx> {
    fn into_call_sim(
        self,
        bytecode: Bytecode,
        input: Bytes,
    ) -> Result<Simulation<ExtCtx, InMemoryDB>>;
}

impl<P, ExtCtx: 'static> CallSimBuilderExt<ExtCtx> for SimulationBuilder<P, ExtCtx, InMemoryDB> {
    fn into_call_sim(
        mut self,
        bytecode: Bytecode,
        input: Bytes,
    ) -> Result<Simulation<ExtCtx, InMemoryDB>> {
        let fibonacci_address = Address::from_slice(&[1; 20]);
        let account_info = AccountInfo {
            code_hash: bytecode.hash_slow(),
            code: Some(bytecode),
            ..Default::default()
        };

        let mut db = InMemoryDB::default();
        db.insert_account_info(fibonacci_address, account_info);

        let tx_env = TxEnv {
            transact_to: TransactTo::Call(fibonacci_address),
            data: input,
            ..TxEnv::default()
        };

        let ctx = self.ext_ctx.take().ok_or_eyre("No external context")?;
        let reg = self.handler_register.take();
        let evm = utils::evm::make_evm(db, ctx, reg, None);

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

    fn provider_factory(&self) -> &ProviderFactory<DatabaseEnv>;

    fn into_tx_sim(self, tx_hash: B256) -> Result<S>;
    fn into_block_sim(self, block_number: u64, block_chunk: Option<BlockPart>) -> Result<S>;
    fn make_txs_sim(
        self,
        block: &Header,
        txs: Vec<TransactionSigned>, // todo could just take incidces instaed
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
        let pre_res = preexecute_fn
            .map(|f| f(&mut evm))
            .transpose()?
            .unwrap_or_default();
        Ok(Simulation::new(evm, execute_fn).with_pre_execution_res(pre_res))
    }

    fn make_db_at_block(&self, block_number: u64) -> Result<StateProviderCacheDB> {
        let state_provider = Arc::new(
            self.provider_factory()
                .history_by_block_number(block_number - 1)?,
        );
        Ok(CacheDB::new(StateProviderDatabase::new(state_provider)))
    }

    fn make_env(&self, block_header: &Header) -> EnvWithHandlerCfg {
        let chain_id = self.provider_factory().chain_spec().chain.id();
        utils::evm::env_with_handler_cfg(chain_id, block_header)
    }

    fn get_block(&self, block_number: u64) -> Result<Block> {
        self.provider_factory()
            .block(block_number.into())?
            .ok_or_eyre("No block found")
    }

    fn get_tx_with_meta(&self, tx_hash: B256) -> Result<(TransactionSigned, TransactionMeta)> {
        self.provider_factory()
            .transaction_by_hash_with_meta(tx_hash)?
            .ok_or_eyre("No tx found")
    }
}

impl<ExtCtx: 'static> TxsSimBuilderExt<ExtCtx, Simulation<ExtCtx, StateProviderCacheDB>>
    for SimulationBuilder<ProviderFactory<DatabaseEnv>, ExtCtx, StateProviderCacheDB>
{
    fn provider_factory(&self) -> &ProviderFactory<DatabaseEnv> {
        &self.provider_factory
    }

    fn into_tx_sim(self, tx_hash: B256) -> Result<Self::SimType> {
        let (tx, meta) = self.get_tx_with_meta(tx_hash)?;
        let Block { body, header, .. } = self.get_block(meta.block_number)?;
        let pre_execution_txs = body[..(meta.index as usize)].to_vec();
        self.make_txs_sim(&header, vec![tx], pre_execution_txs)
    }

    fn into_block_sim(
        self,
        block_number: u64,
        block_chunk: Option<BlockPart>,
    ) -> Result<Self::SimType> {
        let Block { body, header, .. } = self.get_block(block_number)?;
        let (txs, pre_execution) = match block_chunk {
            Some(chunk) => chunk.split_txs(body),
            None => (body, vec![]),
        };
        self.make_txs_sim(&header, txs, pre_execution)
    }

    fn make_txs_sim(
        mut self,
        block_header: &Header,
        tx_hashes: Vec<TransactionSigned>,
        pre_execution_txs: Vec<TransactionSigned>,
    ) -> Result<Self::SimType> {
        let db = self.make_db_at_block(block_header.number)?;
        let env = self.make_env(block_header);
        let evm = utils::evm::make_evm(
            db,
            self.ext_ctx.take().ok_or_eyre("No external context")?,
            self.handler_register.take(),
            Some(env),
        );

        let execute_fn = Box::new(move |evm: &mut SimEvm<ExtCtx>| {
            tx_sim::sim_txs(&tx_hashes, evm).map(|r| r.into_sim_results())
        });
        let preexecute_fn = Box::new(|evm: &mut SimEvm<ExtCtx>| {
            tx_sim::sim_txs(&pre_execution_txs, evm).map(|r| r.into_sim_results())
        });

        Self::make_sim(evm, execute_fn, Some(preexecute_fn))
    }
}

pub struct SimulationBuilder<P, ExtCtx, DB: Database> {
    provider_factory: P,
    ext_ctx: Option<ExtCtx>,
    handler_register: Option<HandleRegister<ExtCtx, DB>>,
    _db: std::marker::PhantomData<DB>,
}

impl<DB: Database + DatabaseRef> Default for SimulationBuilder<(), (), DB> {
    fn default() -> Self {
        Self {
            provider_factory: (),
            ext_ctx: None,
            handler_register: None,
            _db: std::marker::PhantomData,
        }
    }
}

impl<P, ExtCtx, DB: Database> SimulationBuilder<P, ExtCtx, DB> {
    pub fn with_ext_ctx<ExtCtxInner>(
        self,
        ext_ctx: ExtCtxInner,
    ) -> SimulationBuilder<P, ExtCtxInner, DB> {
        SimulationBuilder {
            provider_factory: self.provider_factory,
            handler_register: None,
            ext_ctx: Some(ext_ctx),
            _db: self._db,
        }
    }

    pub fn with_handle_register(self, handle_register: HandleRegister<ExtCtx, DB>) -> Self {
        Self {
            provider_factory: self.provider_factory,
            handler_register: Some(handle_register),
            ext_ctx: self.ext_ctx,
            _db: self._db,
        }
    }
}

impl<ExtCtx, DB: Database> SimulationBuilder<(), ExtCtx, DB> {
    pub fn with_provider_factory(
        self,
        provider_factory: ProviderFactory<DatabaseEnv>,
    ) -> SimulationBuilder<ProviderFactory<DatabaseEnv>, ExtCtx, DB> {
        SimulationBuilder {
            ext_ctx: None,
            handler_register: None,
            provider_factory,
            _db: self._db,
        }
    }
}

pub struct Simulation<ExtCtx: 'static, DB: Database + DatabaseRef + 'static> {
    evm: Evm<'static, ExtCtx, DB>,
    fnc: SimFn<'static, ExtCtx, DB>,
    pre_execution_res: Option<Vec<SimResult>>,
}

impl<ExtCtx, DB> Simulation<ExtCtx, DB>
where
    DB: Database + DatabaseRef + Clone,
{
    pub fn new(evm: Evm<'static, ExtCtx, DB>, fnc: SimFn<'static, ExtCtx, DB>) -> Self {
        Self {
            evm,
            fnc,
            pre_execution_res: None,
        }
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

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub enum BlockPart {
    TOB(f32),
    BOB(f32),
}

impl BlockPart {
    pub fn split_txs<T>(&self, mut txs: Vec<T>) -> (Vec<T>, Vec<T>) {
        let mut pre_execution = vec![];
        match self {
            BlockPart::TOB(chunk) => {
                let chunk_size = (txs.len() as f32 * chunk).ceil() as usize;
                txs = txs.into_iter().take(chunk_size).collect();
            }
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
