use super::build::{self, CompilerOptions, CompileArgs};
use reth_db::{open_db_read_only, DatabaseEnv};
use reth_provider::{
    providers::StaticFileProvider,
    ProviderFactory, 
    StateProvider,
};
use reth_chainspec::ChainSpecBuilder;
use reth_primitives::Address;
use std::iter::IntoIterator;

use std::sync::Arc;
use std::path::Path;
use eyre::{OptionExt, Result};


pub struct CompileArgsWithAddress {
    pub address: Address,
    pub label: String,
    pub options: Option<CompilerOptions>,
}

impl CompileArgsWithAddress {

    fn into_compile_args(self, state_provider: &impl StateProvider) -> Result<CompileArgs> {
        let code = state_provider.account_code(self.address)?
            .ok_or_eyre("No code found for address")?;
        Ok(CompileArgs {
            label: self.label,
            code,
            options: self.options,
        })
    }
}

pub fn compile_contracts_with_address(
    state_provider: Arc<impl StateProvider>,
    contracts: impl IntoIterator<Item=CompileArgsWithAddress>
) -> Result<Vec<Result<()>>> {
    let contracts = contracts.into_iter()
        .map(|c| c.into_compile_args(&state_provider))
        .collect::<Result<Vec<_>>>()?;
    let results = build::compile_contracts(contracts);
    Ok(results)
}


pub fn make_provider_factory(db_path: &str) -> Result<ProviderFactory<DatabaseEnv>> {
    let db_path = Path::new(db_path);
    let db = open_db_read_only(db_path.join("db").as_path(), Default::default())?;

    let spec = ChainSpecBuilder::mainnet().build();
    let stat_file_provider = StaticFileProvider::read_only(db_path.join("static_files"))?;
    let factory = ProviderFactory::new(db, spec.into(), stat_file_provider);

    Ok(factory)
}
