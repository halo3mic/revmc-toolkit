// todo: track storage reads and writes to discount them from total gas used
// todo: add option to ignore entries with big deviation from the mid block (inconsistent gas usage)

use std::{collections::{HashMap, VecDeque}, ops::AddAssign};
use eyre::{Ok, Result};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use reth_db::DatabaseEnv;
use reth_provider::{
    BlockNumReader, ProviderFactory, StateProvider,
};
use revm::{
    interpreter::{CallInputs, CallOutcome},
    primitives::{Address, B256, Bytecode},
    EvmContext, 
    Inspector
};
use revmc_toolkit_utils as utils;
use crate::sim_builder::{self, TxsSimBuilderExt, StateProviderCacheDB};


#[derive(Default, Debug, Clone)]
pub struct ContractUsage {
    pub gas_used: u64,
    pub frequency: u64,
    pub gas_deficit: u64,
}

impl ContractUsage {

    pub fn gas_used(&self) -> u64 {
        let gas_used = self.gas_used as i64 - self.gas_deficit as i64;
        if gas_used < 0 {
            panic!("Negative gas used")
        }
        gas_used as u64
    }

    pub fn frequency(&self) -> u64 {
        self.frequency
    }

    fn merge(&mut self, other: &Self) {
        self.frequency += other.frequency;
        self.gas_used += other.gas_used;
        self.gas_deficit += other.gas_deficit;
    }

    fn update(&mut self, gas_used: u64) {
        self.frequency += 1;
        self.gas_used += gas_used;
    }
}

#[derive(Default)]
struct BytecodeContractUsageInspector {
    account_to_usage: HashMap<Address, ContractUsage>,
    parent_bytecode_stack: VecDeque<Address>,
    current_bytecode: Option<Address>,
}

impl BytecodeContractUsageInspector {
    
    fn record_usage(&mut self, contract: Address, gas_used: u64) {
        let entry = self.account_to_usage
            .entry(contract)
            .or_default();
        entry.update(gas_used);
    }

    fn record_gas_deficit(&mut self, caller: Address, gas_used: u64) {
        let entry = self.account_to_usage
            .entry(caller)
            .or_default();
        entry.gas_deficit += gas_used;
    }

}

impl<DB: revm::Database> Inspector<DB> for BytecodeContractUsageInspector {

    fn call(
        &mut self,
        _context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        if let Some(bytecode_address) = self.current_bytecode {
            self.parent_bytecode_stack.push_back(bytecode_address);
        }
        self.current_bytecode = Some(inputs.bytecode_address);
        None
    }

    fn call_end(
        &mut self,
        _context: &mut EvmContext<DB>,
        inputs: &CallInputs,
        outcome: CallOutcome,
    ) -> CallOutcome {
        let contract = inputs.bytecode_address; // We care about which bytecode
        let gas_used = outcome.gas().spent();

        self.record_usage(contract, gas_used);

        if let Some(parent_bytecode) = self.parent_bytecode_stack.pop_back() {
            self.record_gas_deficit(parent_bytecode, gas_used);
            self.current_bytecode = Some(parent_bytecode);
        } else {
            self.current_bytecode = None;
        }

        outcome
    }

}

pub struct GasGuzzlerBytecodeUsage {
    pub contracts: HashMap<Address, u64>,
    pub usage: ContractUsage,
}

impl GasGuzzlerBytecodeUsage {

    fn new(contract: Address, usage: ContractUsage) -> Self {
        let contracts = [(contract, usage.frequency)].into_iter().collect();
        Self { contracts, usage }
    }

    fn update(&mut self, contract: Address, usage: &ContractUsage) {
        self.contracts
            .entry(contract)
            .and_modify(|e| *e += usage.frequency)
            .or_insert(usage.frequency);
        self.usage.merge(usage);
    }
}

#[derive(serde::Serialize)]
pub struct BytecodeStat<T> {
    pub bytecode: T,
    pub gas_used: u64,
    pub frequency: u64,
    pub prop_gas_used: f64,
    pub prop_frequency: f64,
    pub csum_prop_gas_used: f64,
    pub csum_prop_frequency: f64,
    pub most_used_address: Option<Address>,
}

impl BytecodeStat<Bytecode> {
    pub fn bytecode_to_hash(self) -> BytecodeStat<B256>  {
        let hash = self.bytecode.hash_slow();
        BytecodeStat {
            bytecode: hash,
            ..self
        }
    }
}

pub struct GasGuzzlerReport {
    pub csum_stats: ContractUsage,
    pub bytecode_stats: HashMap<Vec<u8>, GasGuzzlerBytecodeUsage>,
}

impl GasGuzzlerReport {
    pub fn new(
        usage: MapWrapper<Address, ContractUsage>,
        state_provider: &Box<dyn StateProvider>,
    ) -> Result<Self> {
        let mut bytecode_stats: HashMap<_, GasGuzzlerBytecodeUsage> = HashMap::new();
        let mut csum_stats = ContractUsage::default();
        for (contract, usage) in usage.0.into_iter() {
            let bytecode = Self::bytecode_for_contract(contract, state_provider)?;
            if let Some(bytecode) = bytecode {
                csum_stats.merge(&usage);
                bytecode_stats
                    .entry(bytecode)
                    .and_modify(|entry| entry.update(contract, &usage))
                    .or_insert_with(|| GasGuzzlerBytecodeUsage::new(contract, usage));
            }
        }
        Ok(Self {
            csum_stats,
            bytecode_stats,
        })
    }

    pub fn into_top_guzzlers_stats(self, take_size: Option<usize>) -> Vec<BytecodeStat<Bytecode>> {
        let GasGuzzlerReport { csum_stats, bytecode_stats } = self;
        let take_size = take_size.unwrap_or(csum_stats.frequency() as usize);
        let mut parsed = bytecode_stats
            .into_iter()
            .map(|(bytecode, usage)| {
                let most_used_address = usage.contracts.into_iter()
                    .max_by_key(|(_, freq)| *freq)
                    .map(|(addr, _)| addr);

                let gas_used = usage.usage.gas_used();
                let prop_gas_used = gas_used as f64 / csum_stats.gas_used() as f64;
                
                let frequency = usage.usage.frequency();
                let prop_frequency = frequency as f64 / csum_stats.frequency() as f64;

                (bytecode, most_used_address, gas_used, frequency, prop_gas_used, prop_frequency)
            })
            .collect::<Vec<_>>();

        parsed.sort_by(|a, b| b.4.partial_cmp(&a.4).unwrap());

        parsed
            .into_iter()
            .take(take_size)
            .scan((0., 0.), |(gas_used, freq), elements| {
                *gas_used += elements.4;
                *freq += elements.5;
                Some(BytecodeStat {
                    bytecode: Bytecode::new_raw(elements.0.into()),
                    gas_used: elements.2,
                    frequency: elements.3,
                    prop_gas_used: elements.4,
                    prop_frequency: elements.5,
                    csum_prop_gas_used: *gas_used,
                    csum_prop_frequency: *freq,
                    most_used_address: elements.1,
                })
            })
            .collect()
    }

    pub fn into_top_guzzlers(self, take_size: Option<usize>) -> Vec<Vec<u8>> {
        self.into_top_guzzlers_stats(take_size)
            .into_iter()
            .map(|e| e.bytecode.bytes_slice().to_vec())
            .collect()
    }

    fn bytecode_for_contract(contract: Address, state_provider: &Box<dyn StateProvider>) -> Result<Option<Vec<u8>>> {
        Ok(state_provider.account_code(contract)?
            .map(|code| code.original_bytes().into()))
    }

}

#[derive(Default)]
pub struct MapWrapper<K, V>(HashMap<K, V>);

impl<K, V> MapWrapper<K, V> {
    pub fn new() -> Self {
        Self(HashMap::new())
    }
    pub fn into_inner(self) -> HashMap<K, V> {
        self.0
    }
}

impl<K: Eq + std::hash::Hash> MapWrapper<K, ContractUsage> {

    fn join(&mut self, key: K, value: ContractUsage) {
        self.0.entry(key)
            .and_modify(|e| e.merge(&value))
            .or_insert(value);
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
    pub start_block: Option<u64>,
    pub end_block: Option<u64>,
    pub sample_size: Option<u64>,
    pub seed: Option<[u8; 32]>,
}

impl GasGuzzlerConfig {

    pub fn with_start_block(mut self, start_block: u64) -> Self {
        self.start_block = Some(start_block);
        self
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
        provider_factory: ProviderFactory<DatabaseEnv>,
    ) -> Result<GasGuzzlerReport> {
        let end_block = self.end_block.unwrap_or(provider_factory.last_block_number()?);
        let start_block = self.start_block.unwrap_or(end_block-10_000);
        let sample_size = self.sample_size.unwrap_or((end_block-start_block)/10);
        let sample_iter = utils::rnd::random_sequence(start_block, end_block, sample_size as usize, self.seed)?;
        
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
            .reduce(|| Ok(MapWrapper::new()), |acc, item| {
                let mut acc = acc?;
                acc += item?;
                Ok(acc)
            })?;

        Ok(GasGuzzlerReport::new(
            contract_usage, 
            &provider_factory.latest()?
        )?)
    
    }

    fn make_sim_for_block(
        provider_factory: ProviderFactory<DatabaseEnv>,
        block_num: u64,
    ) -> Result<sim_builder::Simulation<BytecodeContractUsageInspector, StateProviderCacheDB>> {
        sim_builder::SimulationBuilder::default()
            .with_provider_factory(provider_factory)
            .with_ext_ctx(BytecodeContractUsageInspector::default())
            .with_handle_register(revm::inspector_handle_register)
            .into_block_sim(block_num, None)
    }

}