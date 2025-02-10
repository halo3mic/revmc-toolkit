use reth_db::DatabaseEnv;
use reth_provider::ProviderFactory;
use revm::{
    primitives::{hex, keccak256, Bytecode, Bytes, B256, U256},
    InMemoryDB,
};

use eyre::Result;
use std::str::FromStr;

use revmc_toolkit_build::CompilerOptions;
use revmc_toolkit_load::{
    revmc_register_handler, EvmCompilerFnLoader, EvmCompilerFns, RevmcExtCtx,
};
use revmc_toolkit_sim::sim_builder::{
    self, BlockPart, CallSimBuilderExt, Simulation, StateProviderCacheDB, TxsSimBuilderExt,
};
use revmc_toolkit_sim::{bytecode_touches, gas_guzzlers::GasGuzzlerConfig};

pub struct SimConfig<P> {
    ext_ctx: RevmcExtCtx,
    provider_factory: P,
}

impl From<RevmcExtCtx> for SimConfig<()> {
    fn from(ext_ctx: RevmcExtCtx) -> Self {
        SimConfig {
            ext_ctx,
            provider_factory: (),
        }
    }
}

impl<P> SimConfig<P> {
    pub fn make_call_sim(
        &self,
        call_type: SimCall,
        input: Bytes,
    ) -> Result<Simulation<RevmcExtCtx, InMemoryDB>> {
        let sim = sim_builder::SimulationBuilder::default()
            .with_ext_ctx(self.ext_ctx.clone())
            .with_handle_register(revmc_register_handler)
            .into_call_sim(call_type.bytecode(), input)?;
        Ok(sim)
    }
}

impl SimConfig<ProviderFactory<DatabaseEnv>> {
    pub fn new(provider_factory: ProviderFactory<DatabaseEnv>, ext_ctx: RevmcExtCtx) -> Self {
        Self {
            provider_factory,
            ext_ctx,
        }
    }

    pub fn make_tx_sim(
        &self,
        tx_hash: B256,
    ) -> Result<Simulation<RevmcExtCtx, StateProviderCacheDB>> {
        let sim = sim_builder::SimulationBuilder::default()
            .with_provider_factory(self.provider_factory.clone())
            .with_ext_ctx(self.ext_ctx.clone())
            .with_handle_register(revmc_register_handler)
            .into_tx_sim(tx_hash)?;
        Ok(sim)
    }

    pub fn make_block_sim(
        &self,
        block_num: u64,
        block_part: Option<BlockPart>,
    ) -> Result<Simulation<RevmcExtCtx, StateProviderCacheDB>> {
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
            "jit" => Ok(SimRunType::JITCompiled),
            "aot" => Ok(SimRunType::AOTCompiled),
            _ => Err(eyre::eyre!("Invalid run type")),
        }
    }
}

pub fn make_ext_ctx(
    run_type: &SimRunType,
    bytecodes: &[Vec<u8>],
    compile_opt: Option<CompilerOptions>,
) -> Result<RevmcExtCtx> {
    make_compiled_fns(run_type, bytecodes, compile_opt).map(Into::into)
}

pub fn make_compiled_fns(
    run_type: &SimRunType,
    bytecodes: &[Vec<u8>],
    compile_opt: Option<CompilerOptions>,
) -> Result<EvmCompilerFns> {
    Ok(match run_type {
        SimRunType::Native => EvmCompilerFns::default(),
        SimRunType::JITCompiled => {
            revmc_toolkit_build::compile_contracts_jit(bytecodes, compile_opt)?.into()
        }
        SimRunType::AOTCompiled => {
            let compile_opt = compile_opt.unwrap_or_default();
            let aot_out_dir = compile_opt.out_dir.clone();
            let bytecode_hashes = bytecodes.iter().map(keccak256).collect();
            revmc_toolkit_build::compile_contracts_aot(bytecodes, Some(compile_opt))?
                .into_iter()
                .collect::<Result<Vec<_>>>()?;
            EvmCompilerFnLoader::new(&aot_out_dir)
                .load_selected(bytecode_hashes)
                .into()
        }
    })
}

#[derive(serde::Serialize)]
pub enum BytecodeSelection {
    Selected {
        blacklist: Vec<B256>,
    },
    GasGuzzlers {
        config: GasGuzzlerConfig,
        size_limit: usize,
        blacklist: Vec<B256>,
    },
}

impl BytecodeSelection {
    pub fn bytecodes(
        &self,
        provider_factory: ProviderFactory<DatabaseEnv>,
        med: Option<BytecodeTouchMediums>,
    ) -> Result<Vec<Vec<u8>>> {
        let mut bytecodes = match self {
            BytecodeSelection::Selected { .. } => {
                let med = med.ok_or(eyre::eyre!("Missing hashes/blocks"))?;
                match med {
                    BytecodeTouchMediums::Txs(txs) => {
                        tracing::info!("Finding touched bytecode for selected txs");
                        bytecode_touches::find_touched_bytecode(provider_factory, txs)?
                            .into_iter()
                            .collect()
                    }
                    BytecodeTouchMediums::Blocks(blocks) => {
                        tracing::info!("Finding touched bytecode for selected blocks");
                        bytecode_touches::find_touched_bytecode_blocks(provider_factory, blocks)?
                            .into_iter()
                            .collect()
                    }
                }
            }
            BytecodeSelection::GasGuzzlers {
                config, size_limit, ..
            } => {
                tracing::info!("Finding gas guzzlers");
                config
                    .find_gas_guzzlers(provider_factory)?
                    .into_top_guzzlers(Some(*size_limit))
            }
        };
        let blacklist = self.blacklist();
        if !blacklist.is_empty() {
            bytecodes.retain(|bc| !blacklist.contains(&keccak256(bc)));
        }
        Ok(bytecodes)
    }

    fn blacklist(&self) -> Vec<B256> {
        match self {
            BytecodeSelection::Selected { blacklist } => blacklist.clone(),
            BytecodeSelection::GasGuzzlers { blacklist, .. } => blacklist.clone(),
        }
    }
}

impl Default for BytecodeSelection {
    fn default() -> Self {
        Self::Selected {
            blacklist: Vec::new(),
        }
    }
}

pub enum BytecodeTouchMediums<'a> {
    Txs(Vec<B256>),
    Blocks(&'a Vec<u64>),
}

impl<'a> From<&'a Vec<u64>> for BytecodeTouchMediums<'a> {
    fn from(v: &'a Vec<u64>) -> Self {
        Self::Blocks(v)
    }
}

impl<'a> From<Vec<B256>> for BytecodeTouchMediums<'a> {
    fn from(v: Vec<B256>) -> Self {
        Self::Txs(v)
    }
}

// Call simulation

#[derive(Clone, Copy, Debug)]
pub enum SimCall {
    Fibbonacci,
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
    pub fn default_input(&self) -> Bytes {
        match self {
            SimCall::Fibbonacci => U256::from(100_000).to_be_bytes_vec().into(),
        }
    }
}
