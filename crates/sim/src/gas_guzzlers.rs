use std::collections::HashMap;
use std::ops::AddAssign;
use eyre::{Ok, Result};
use std::sync::Arc;
use tracing::warn;
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use reth_db::DatabaseEnv;
use reth_provider::{
    BlockNumReader, ProviderFactory, StateProvider,
};
use revm::interpreter::{CallInputs, CallOutcome};
use revm::{self, EvmContext, Inspector};
use revm::primitives::{Address, Bytes};

use revmc_toolbox_utils as utils;
use crate::sim_builder::{self, TxsSimBuilderExt, StateProviderCacheDB};


// todo: track storage reads and writes to discount them from total gas used
#[derive(Default, Debug, Clone)]
pub struct ContractUsage {
    pub gas_used: u64,
    pub frequency: u64,
    pub first_block: u64,
    pub last_block: u64,
    pub gas_deficit: u64,
    cumm_block: u128,
}

impl ContractUsage {
    fn with_first_block(mut self, first_block: u64) -> Self {
        self.first_block = first_block;
        self
    }

    fn new_usage(&mut self, gas_used: u64, block_num: u64) {
        self.frequency += 1;
        self.gas_used += gas_used;
        self.cumm_block += block_num as u128;
        
        if self.first_block > block_num {
            self.first_block = block_num;
        }
        if self.last_block < block_num {
            self.last_block = block_num;
        }
    }

    pub fn mean_block(&self) -> u64 {
        if self.frequency == 0 {
            0
        } else {
            (self.cumm_block / self.frequency as u128) as u64
        }
    }
}


#[derive(Default)]
struct BytecodeContractUsageInspector {
    account_to_usage: HashMap<Address, ContractUsage>,
}

impl BytecodeContractUsageInspector {
    
    fn record_usage(&mut self, contract: Address, gas_used: u64, block_num: u64) {
        let entry = self.account_to_usage
            .entry(contract)
            .or_insert(ContractUsage::default().with_first_block(block_num));
        entry.new_usage(gas_used, block_num);
    }

    fn record_gas_deficit(&mut self, caller: Address, gas_used: u64, block_num: u64) {
        let entry = self.account_to_usage
            .entry(caller)
            .or_insert(ContractUsage::default().with_first_block(block_num));
        entry.gas_deficit += gas_used;
    }

}

impl<DB: revm::Database> Inspector<DB> for BytecodeContractUsageInspector {

    fn call_end(
        &mut self,
        context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        let contract = inputs.bytecode_address; // We care about which bytecode
        let gas_used = outcome.gas().spent();
        let block_num = context.env.block.number.to();

        self.record_usage(contract, gas_used, block_num);

        let caller =
            if inputs.bytecode_address == inputs.target_address {
                inputs.caller
            } else {
                // delegate call
                inputs.target_address
            };
        self.record_gas_deficit(caller, gas_used, block_num);

        outcome
    }

}

#[derive(Default)]
pub struct MapWrapper<K, V>(HashMap<K, V>);

impl<K, V> MapWrapper<K, V> {
    pub fn into_inner(self) -> HashMap<K, V> {
        self.0
    }
}

impl<K: Eq + std::hash::Hash> MapWrapper<K, ContractUsage> {

    fn join(&mut self, key: K, value: ContractUsage) {
        self.0.entry(key)
            .and_modify(|e| {
                e.frequency += value.frequency;
                e.gas_used += value.gas_used;
                e.cumm_block += value.cumm_block;
                e.gas_deficit += value.gas_deficit;
                if e.first_block > value.first_block {
                    e.first_block = value.first_block;
                }
                if e.last_block < value.last_block {
                    e.last_block = value.last_block;
                }
            })
            .or_insert(value);
    }
}

// todo: add option to ignore entries with big deviation from the mid block (inconsistent gas usage)

pub struct GasGuzzlerResult<K> {
    pub usage: MapWrapper<K, ContractUsage>,
    state_provider: Box<dyn StateProvider>
}

impl<K> GasGuzzlerResult<K> {

    pub fn new(
        usage: MapWrapper<K, ContractUsage>, 
        state_provider: Box<dyn StateProvider>
    ) -> Self {
        Self { usage, state_provider }
    }

    pub fn into_usage(self) -> HashMap<K, ContractUsage> {
        self.usage.0
    }

    pub fn into_top_guzzlers(self, size: usize) -> Vec<K> {
        let mut net_usage = self.usage.0.into_iter()
            .map(|(key, usage)| {
                let net_gas_used = usage.gas_used as i64 - usage.gas_deficit as i64;
                (key, usage, net_gas_used)
            })
            .collect::<Vec<_>>();
        net_usage.sort_by(|a, b| b.2.cmp(&a.2));
        net_usage.into_iter()
            .take(size)
            .map(|(key, _, _)| key)
            .collect()
    }

}

impl GasGuzzlerResult<Address> {
    pub fn contract_to_bytecode(self) -> Result<GasGuzzlerResult<Vec<u8>>> {
        let mut bytecode_map = MapWrapper::default();
        for (contract, usage) in self.usage.into_inner().into_iter() {
            if let Some(bytecode) = self.state_provider.account_code(contract)? {
                bytecode_map.join(bytecode.bytes().into(), usage);
            } else {
                warn!("Code for contract {contract:?} not found")
            }
        }
        Ok(GasGuzzlerResult::new(bytecode_map, self.state_provider))
    }
}

impl<K: Eq + std::hash::Hash> AddAssign for MapWrapper<K, ContractUsage> {
    fn add_assign(&mut self, other: Self) {
        for (key, value) in other.0 {
            self.join(key, value);
        }   
    }
}

#[derive(Debug, Clone, Default)]
pub struct GasGuzzlerConfig {
    pub start_block: u64,
    pub end_block: Option<u64>,
    pub sample_size: Option<u64>,
    pub seed: Option<[u8; 32]>,
}

impl GasGuzzlerConfig {

    pub fn new(start_block: u64) -> Self {
        Self {
            start_block,
            ..Default::default()
        }
    }

    pub fn with_end_block(mut self, end_block: u64) -> Self {
        self.end_block = Some(end_block);
        self
    }

    pub fn with_sample_size(mut self, sample_size: u64) -> Self {
        self.sample_size = Some(sample_size);
        self
    }

    pub fn with_seed(mut self, seed: [u8; 32]) -> Self {
        self.seed = Some(seed);
        self
    }

    pub fn find_gas_guzzlers(
        &self,
        provider_factory: Arc<ProviderFactory<DatabaseEnv>>,
    ) -> Result<GasGuzzlerResult<Address>> {
        let end_block = self.end_block.unwrap_or(provider_factory.last_block_number()?);
        let sample_size = self.sample_size.unwrap_or(end_block-self.start_block);
        let sample_iter = utils::rnd::random_sequence(self.start_block, end_block, sample_size as usize, self.seed)?;
        
        let contract_usage = sample_iter
            .into_par_iter()
            .map(|block_num| {
                let mut sim = Self::make_sim_for_block(
                    provider_factory.clone(), 
                    block_num
                )?;
                sim.run()?;
                let evm = sim.into_evm();
                Ok(MapWrapper(evm.context.external.account_to_usage))
            })
            .reduce(|| Ok(MapWrapper::default()), |acc, item| {
                let mut acc = acc?;
                let item = item?;
                acc += item;
                Ok(acc)
            })?;

        Ok(GasGuzzlerResult::new(
            contract_usage, 
            provider_factory.latest()?
        ))
    
    }

    fn make_sim_for_block(
        provider_factory: Arc<ProviderFactory<DatabaseEnv>>,
        block_num: u64,
    ) -> Result<sim_builder::Simulation<BytecodeContractUsageInspector, StateProviderCacheDB>> {
        sim_builder::SimulationBuilder::default()
            .with_provider_factory(provider_factory)
            .with_ext_ctx(BytecodeContractUsageInspector::default())
            .with_handle_register(revm::inspector_handle_register)
            .into_block_sim(block_num, None)
    }

}


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use revmc_toolbox_utils as utils;

    #[test]
    fn test_gas_guzzlers() -> Result<()>{
        dotenv::dotenv()?;
        let db_path = std::env::var("RETH_DB_PATH")?;
        let db_path = Path::new(&db_path);
        let provider_factory = Arc::new(utils::evm::make_provider_factory(&db_path).unwrap());
        let block = 20392617;
        let end_block = block+200_000;
        let config = GasGuzzlerConfig::new(block)
            .with_end_block(end_block)
            .with_sample_size(1000);
        let gas_guzzlers = config.find_gas_guzzlers(provider_factory.clone())?;

        let mid_block = (block+end_block)/2;
        let mut gas_guzzlers = gas_guzzlers.into_usage().into_iter()
            .map(|(key, usage)| {
                let mean_block = usage.mean_block();
                let mid_block_offset = (mid_block as i64 - mean_block as i64).abs();
                let net_gas_used = usage.gas_used as i64 - usage.gas_deficit as i64;
                (key, usage, mid_block_offset, net_gas_used, (mid_block, mean_block))
            })
            .collect::<Vec<_>>();
        gas_guzzlers.sort_by(|a, b| b.3.cmp(&a.3));

        println!("{:#?}", gas_guzzlers[..1000].to_vec());

        Ok(())
    }

}