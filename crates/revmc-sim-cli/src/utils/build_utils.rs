use revmc_sim_build::{CompilerOptions, CodeWithOptions};
use serde::{Deserialize, Deserializer};
use reth_provider::StateProvider;
use revm::primitives::Address;
use std::iter::IntoIterator;
use std::path::PathBuf;
use serde_json::Value;

use eyre::{OptionExt, Result};

// #[derive(Debug, Clone, serde::Deserialize)]
// pub struct CompileArgsWithAddress {
//     pub address: Option<Address>,
//     pub options: Option<CompilerOptions>,
// }

// impl CompileArgsWithAddress {

//     fn new(address: Address) -> Self {
//         Self { address, options: None }
//     }

//     fn into_code_with_opt(self, state_provider: &impl StateProvider) -> Result<CodeWithOptions> {
//         let code = state_provider.account_code(self.address)?
//             .ok_or_eyre("No code found for address")?;
//         let code_bytes = code.original_byte_slice().to_vec();
//         Ok(CodeWithOptions {
//             code: code_bytes,
//             options: self.options,
//         })
//     }
// }

// pub fn compile_contracts_with_address(
//     state_provider: Box<impl StateProvider + ?Sized>,
//     contracts: impl IntoIterator<Item=CompileArgsWithAddress>,
//     fallback_opt: Option<CompilerOptions>,
// ) -> Result<Vec<Result<()>>> {
//     let contracts = contracts.into_iter()
//         .map(|c| c.into_code_with_opt(&state_provider))
//         .collect::<Result<Vec<_>>>()?;
//     revmc_sim_build::compile_contracts(contracts, fallback_opt)
//     // todo: check all processes are ok
// }

fn fetch_code_for_account(state_provider: &impl StateProvider, account: Address) -> Result<Vec<u8>> {
    let code = state_provider.account_code(account)?
        .ok_or_eyre("No code found for address")?;
    let code_bytes = code.original_byte_slice().to_vec();
    Ok(code_bytes)
}


pub fn compile_contracts(
    state_provider: Box<impl StateProvider + ?Sized>,
    build_file: BuildFile,
) -> Result<Vec<Result<()>>> {
    // todo: pass on the label into config file
    let contracts = build_file.contracts.into_iter()
        .map(|c| {
            if let Some(address) = c.address {
                let code = fetch_code_for_account(&state_provider, address)?;
                Ok(CodeWithOptions { code, options: c.options })
            } else if let Some(code) = c.code {
                Ok(CodeWithOptions { code, options: c.options })
            } else {
                return Err(eyre::eyre!("No address or code found"));
            }
        })
        .collect::<Result<Vec<_>>>()?;
    revmc_sim_build::compile_contracts(contracts, build_file.fallback_config)
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildObject {
    pub address: Option<Address>,
    #[serde(default, deserialize_with = "hex_or_vec")]
    pub code: Option<Vec<u8>>,
    pub options: Option<CompilerOptions>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BuildFile {
    contracts: Vec<BuildObject>, 
    #[serde(rename = "fallbackConfig")]
    fallback_config: Option<CompilerOptions>,
}

pub fn compile_from_file(
    state_provider: Box<impl StateProvider + ?Sized>,
    file_path: &PathBuf,
) -> Result<Vec<Result<()>>> {
    let config_txt = std::fs::read_to_string(file_path)?;
    let build_file = serde_json::from_str(&config_txt)?;
    compile_contracts(state_provider, build_file)
}

fn hex_or_vec<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    
    match value {
        Some(Value::String(s)) => {
            let s = s.trim_start_matches("0x");
            Some(hex::decode(s).map_err(serde::de::Error::custom)).transpose()
        },
        Some(Value::Array(arr)) => Some(arr
            .into_iter()
            .map(|v| v.as_u64().ok_or_else(|| serde::de::Error::custom("Expected a number"))
                .and_then(|n| u8::try_from(n).map_err(serde::de::Error::custom)))
            .collect()).transpose(),
        Some(_) => Err(serde::de::Error::custom("Expected hex string or array of numbers")),
        None => Ok(None),
    }
}