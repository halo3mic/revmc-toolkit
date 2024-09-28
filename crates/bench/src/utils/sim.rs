use revm::{primitives::{Bytecode, Bytes, B256, hex, keccak256}, InMemoryDB};
use reth_provider::ProviderFactory;
use reth_db::DatabaseEnv;

use std::{path::PathBuf, str::FromStr, sync::Arc};
use eyre::Result;

use revmc_toolkit_sim::sim_builder::{
    self, BlockPart, CallSimBuilderExt, Simulation, 
    StateProviderCacheDB, TxsSimBuilderExt,
};
use revmc_toolkit_load::{EvmCompilerFnLoader, RevmcExtCtx, revmc_register_handler};
use revmc_toolkit_utils::build as build_utils;

pub struct SimConfig<P> {
    ext_ctx: RevmcExtCtx, 
    provider_factory: P,
}

impl From<RevmcExtCtx> for SimConfig<()> {
    fn from(ext_ctx: RevmcExtCtx) -> Self {
        SimConfig { ext_ctx, provider_factory: () }
    }
}

impl<P> SimConfig<P> {
    pub fn make_call_sim(&self, call_type: SimCall, input: Bytes) -> Result<Simulation<RevmcExtCtx, InMemoryDB>> {
        let sim = sim_builder::SimulationBuilder::default()
            .with_ext_ctx(self.ext_ctx.clone())
            .with_handle_register(revmc_register_handler)
            .into_call_sim(call_type.bytecode(), input)?;
        Ok(sim)
    }
}

impl SimConfig<ProviderFactory<DatabaseEnv>> {

    pub fn new(provider_factory: ProviderFactory<DatabaseEnv>, ext_ctx: RevmcExtCtx) -> Self {
        Self { provider_factory, ext_ctx }
    }

    pub fn make_tx_sim(&self, tx_hash: B256) -> Result<Simulation<RevmcExtCtx, StateProviderCacheDB>> {
        let sim = sim_builder::SimulationBuilder::default()
            .with_provider_factory(self.provider_factory.clone())
            .with_ext_ctx(self.ext_ctx.clone())
            .with_handle_register(revmc_register_handler)
            .into_tx_sim(tx_hash)?;
        Ok(sim)
    }
    
    pub fn make_block_sim(&self, block_num: u64, block_part: Option<BlockPart>) -> Result<Simulation<RevmcExtCtx, StateProviderCacheDB>> {
        let sim = sim_builder::SimulationBuilder::default()
            .with_provider_factory(self.provider_factory.clone())
            .with_ext_ctx(self.ext_ctx.clone())
            .with_handle_register(revmc_register_handler)
            .into_block_sim(block_num, block_part)?;
        Ok(sim)
    }

}

// Sim exe types

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
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
            "jit_compiled" => Ok(SimRunType::JITCompiled),
            "aot_compiled" => Ok(SimRunType::AOTCompiled),
            _ => Err(eyre::eyre!("Invalid run type")),
        }
    }
}

pub fn make_ext_ctx<'a>(
    run_type: SimRunType, 
    bytecode: Vec<Vec<u8>>, 
    aot_dir: Option<&PathBuf>,
) -> Result<RevmcExtCtx> {
    Ok(match run_type {
        SimRunType::Native => {
            RevmcExtCtx::default()
        }
        SimRunType::JITCompiled => {
            build_utils::compile_jit_from_codes(bytecode, None)?
                .into_iter()
                .collect::<Result<Vec<_>>>()?
                .into()
        }
        SimRunType::AOTCompiled => {
            let bytecode_hashes = bytecode.iter().map(|code| keccak256(&code)).collect();
            build_utils::compile_aot_from_codes(bytecode, None)?
                .into_iter()
                .collect::<Result<Vec<_>>>()?;
            let aot_dir = aot_dir.ok_or_else(|| eyre::eyre!("AOT dir not provided"))?;
            EvmCompilerFnLoader::new(aot_dir)
                .load_selected(bytecode_hashes)?
                .into()
        }
    })
}

use revmc_toolkit_sim::{gas_guzzlers::GasGuzzlerConfig, bytecode_touches};

pub enum BytecodeSelection {
    Selected, 
    GasGuzzlers { 
        config: GasGuzzlerConfig, 
        size_limit: usize
    },
}
// todo: this shouldnt be together
impl BytecodeSelection {
    pub fn bytecodes(
        &self, 
        provider_factory: ProviderFactory<DatabaseEnv>,
        txs: Option<Vec<B256>>,
    ) -> Result<Vec<Vec<u8>>> {
        Ok(match self {
            BytecodeSelection::Selected => {
                let txs = txs.ok_or(eyre::eyre!("Missing transaction hashes"))?;
                bytecode_touches::find_touched_bytecode(provider_factory, txs)?
                    .into_iter().collect()
            }
            BytecodeSelection::GasGuzzlers { config, size_limit }  => {
                config.find_gas_guzzlers(provider_factory)?
                    .contract_to_bytecode()?
                    .into_top_guzzlers(*size_limit)
            }
        })
    }
    
}

// Call simulation

#[derive(Clone, Copy, Debug)]
pub enum SimCall {
    Fibbonacci
}

impl FromStr for SimCall {
    type Err = eyre::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "fibonacci" => Ok(SimCall::Fibbonacci),
            _ => Err(eyre::eyre!("Invalid call type")),
        }
    }
}

const FIBONACCI_CODE: &[u8] =
    &hex!("5f355f60015b8215601a578181019150909160019003916005565b9150505f5260205ff3");

impl SimCall {
    pub fn bytecode(&self) -> Bytecode {
        match self {
            SimCall::Fibbonacci => Bytecode::new_raw(FIBONACCI_CODE.into()),
        }
    }
}