use std::{collections::HashSet, sync::Arc};
use tracing::warn;
use eyre::Result;
use reth_provider::{StateProvider, ProviderFactory};
use revm::interpreter::{CallInputs, CallOutcome};
use revm::primitives::{Address, Bytes, B256};
use revm::{self, EvmContext, Inspector};
use reth_db::DatabaseEnv;

use crate::sim_builder::{self, TxsSimBuilderExt};


#[derive(Default)]
struct BytecodeTouchInspector {
    touches: HashSet<Address>,
}

impl BytecodeTouchInspector {
    pub fn record_touch(&mut self, address: Address) {
        self.touches.insert(address);
    }
}

impl<DB: revm::Database> Inspector<DB> for BytecodeTouchInspector {
    fn call(
        &mut self,
        _context: &mut EvmContext<DB>,
        inputs: &mut CallInputs,
    ) -> Option<CallOutcome> {
        self.record_touch(inputs.bytecode_address);
        None
    }
}

pub fn find_touched_bytecode(
    provider_factory: Arc<ProviderFactory<DatabaseEnv>>, 
    txs: Vec<B256>,
) -> Result<HashSet<Vec<u8>>> {
    let mut touched_bytecode = HashSet::new();
    for tx_hash in txs {
        let mut sim = sim_builder::SimulationBuilder::default()
            .with_provider_factory(provider_factory.clone())
            .with_ext_ctx(BytecodeTouchInspector::default())
            .into_tx_sim(tx_hash)?;
        sim.run()?;
        let BytecodeTouchInspector { touches } = sim.into_evm().context.external;
        let touched = contracts_to_bytecode(provider_factory.latest()?, touches)?
            .map(|code| code.0.into())
            .collect::<Vec<_>>();
        touched_bytecode.extend(touched);
    }
    Ok(touched_bytecode)
}

use std::collections::hash_set::IntoIter;
use std::iter::IntoIterator;

fn contracts_to_bytecode<T: IntoIterator<Item = Address>>(
    state_provider: Box<dyn StateProvider>, 
    contracts: T
) -> Result<IntoIter<Bytes>> {
    let mut bytecodes = HashSet::new();
    for address in contracts {
        let code = state_provider.account_code(address)?;
        if let Some(code) = code {
            bytecodes.insert(code.bytes());
        } else {
            warn!("Code for contract {address:?} not found")
        }
    }
    Ok(bytecodes.into_iter())
}