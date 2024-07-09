use revmc_sim_build::{CompilerOptions, CodeWithOptions};
use reth_provider::StateProvider;
use revm::primitives::Address;
use std::iter::IntoIterator;
use std::path::PathBuf;

use eyre::{OptionExt, Result};

// todo: make config file compatible with both `code` and `address` fields

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CompileArgsWithAddress {
    pub address: Address,
    pub options: Option<CompilerOptions>,
}

impl CompileArgsWithAddress {

    fn new(address: Address) -> Self {
        Self { address, options: None }
    }

    fn into_code_with_opt(self, state_provider: &impl StateProvider) -> Result<CodeWithOptions> {
        let code = state_provider.account_code(self.address)?
            .ok_or_eyre("No code found for address")?;
        Ok(CodeWithOptions {
            code: code.bytes_slice().to_vec(),
            options: self.options,
        })
    }
}

pub fn compile_contracts_with_address(
    state_provider: Box<impl StateProvider + ?Sized>,
    contracts: impl IntoIterator<Item=CompileArgsWithAddress>,
    fallback_opt: Option<CompilerOptions>,
) -> Result<Vec<Result<()>>> {
    let contracts = contracts.into_iter()
        .map(|c| c.into_code_with_opt(&state_provider))
        .collect::<Result<Vec<_>>>()?;
    let results = revmc_sim_build::compile_contracts(contracts, fallback_opt);
    Ok(results)
}

// #[derive(Debug, Clone, serde::Deserialize)]
// pub struct BuildObject {
//     pub address: Option<Address>,
//     pub code: Option<Vec<u8>>,
//     pub options: Option<CompilerOptions>,
//     pub label: Option<String>,
// }

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BuildFile {
    contracts: Vec<CompileArgsWithAddress>, 
    #[serde(rename = "fallbackConfig")]
    fallback_config: Option<CompilerOptions>,
}

pub fn compile_from_file(
    state_provider: Box<impl StateProvider + ?Sized>,
    file_path: &PathBuf,
) -> Result<Vec<Result<()>>> {
    let config_txt = std::fs::read_to_string(file_path)?;
    let BuildFile { contracts, fallback_config } = serde_json::from_str(&config_txt)?;
    compile_contracts_with_address(state_provider, contracts, fallback_config)
}

