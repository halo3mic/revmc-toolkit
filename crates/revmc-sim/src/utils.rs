use super::build::{self, CompilerOptions, CodeWithOptions};
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
    pub options: Option<CompilerOptions>,
}

impl CompileArgsWithAddress {

    fn new(address: Address) -> Self {

        Self {
            address,
            options: None,
        }
    }

    fn into_code_with_opt(self, state_provider: &impl StateProvider) -> Result<CodeWithOptions> {
        let code = state_provider.account_code(self.address)?
            .ok_or_eyre("No code found for address")?;
        Ok(CodeWithOptions {
            code,
            options: self.options,
        })
    }
}

pub fn compile_contracts_with_address(
    state_provider: Arc<impl StateProvider>,
    contracts: impl IntoIterator<Item=CompileArgsWithAddress>,
    fallback_opt: Option<CompilerOptions>,
) -> Result<Vec<Result<()>>> {
    let contracts = contracts.into_iter()
        .map(|c| c.into_code_with_opt(&state_provider))
        .collect::<Result<Vec<_>>>()?;
    let results = build::compile_contracts(contracts, fallback_opt);
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
