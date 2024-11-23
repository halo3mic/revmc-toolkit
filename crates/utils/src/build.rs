pub use revmc_toolkit_build::{CompilerOptions, JitCompileOut};
use serde::{Deserialize, Deserializer};
use reth_provider::StateProvider;
use revm::primitives::Address;
use std::iter::IntoIterator;
use serde_json::Value;
use std::path::PathBuf;
use eyre::{OptionExt, Result};


pub fn compile_aot_from_contracts_with_fn<F>(
    account_to_code_fn: F,
    contracts: &[Address],
    fallback_opt: Option<CompilerOptions>,
) -> Result<Vec<Result<()>>> 
where F: Fn(&Address) -> Result<Vec<u8>> {
    let contracts = contracts
        .iter()
        .map(|a| account_to_code_fn(a))
        .collect::<Result<Vec<_>>>()?;
    revmc_toolkit_build::compile_contracts_aot(&contracts, fallback_opt)
}

pub fn compile_aot_from_codes(
    codes: &[Vec<u8>],
    fallback_opt: Option<CompilerOptions>,
) -> Result<Vec<Result<()>>>
{
    revmc_toolkit_build::compile_contracts_aot(codes, fallback_opt)
}

pub fn compile_jit_from_codes(
    codes: &[Vec<u8>],
    fallback_opt: Option<CompilerOptions>,
) -> Result<JitCompileOut> {
    revmc_toolkit_build::compile_contracts_jit(codes, fallback_opt)
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct ConfigFile {
    pub fallback_config: Option<CompilerOptions>,
    pub contracts: Vec<CodeWithOptions>,
}

impl ConfigFile {
    pub fn from_path(config_path: PathBuf) -> Result<Self> {
        let config_txt = std::fs::read_to_string(config_path)?;
        let config = serde_json::from_str(&config_txt)?;
        Ok(config)
    }
}

pub fn compile_aot_from_file_path(
    state_provider: &Box<impl StateProvider + ?Sized>,
    file_path: &PathBuf,
) -> Result<Vec<Result<()>>> {
    let config_txt = std::fs::read_to_string(file_path)?;
    let build_file = serde_json::from_str(&config_txt)?;
    compile_aot_from_build_file(state_provider, build_file)
}

pub fn compile_aot_from_build_file(
    state_provider: &Box<impl StateProvider + ?Sized>,
    build_file: BuildFile,
) -> Result<Vec<Result<()>>> {
    let (contracts, fconfig) = build_file.into_contracts_and_fconfig(state_provider)?;
    revmc_toolkit_build::compile_contracts_aot(&contracts, fconfig)
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct CodeWithOptions {
    pub code: Vec<u8>,
    pub options: Option<CompilerOptions>,
}

impl From<Vec<u8>> for CodeWithOptions {
    fn from(code: Vec<u8>) -> Self {
        Self { code, options: None }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildObject {
    pub address: Option<Address>,
    #[serde(default, deserialize_with = "hex_or_vec_opt")]
    pub code: Option<Vec<u8>>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct BuildFile {
    contracts: Vec<BuildObject>, 
    #[serde(rename = "fallbackConfig")]
    compiler_config: Option<CompilerOptions>,
}

impl BuildFile {

    pub fn into_contracts_and_fconfig(
        self, 
        state_provider: &Box<impl StateProvider + ?Sized>
    ) -> Result<(Vec<Vec<u8>>,  Option<CompilerOptions>)> {
        let contracts = self.contracts.into_iter()
            .map(|c| {
                if let Some(address) = c.address {
                    let code = fetch_code_for_account(state_provider, address)?;
                    Ok(code)
                } else if let Some(code) = c.code {
                    Ok(code)
                } else {
                    return Err(eyre::eyre!("No address or code found"));
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok((contracts, self.compiler_config))
    }

}

fn fetch_code_for_account(state_provider: &impl StateProvider, account: Address) -> Result<Vec<u8>> {
    let code = state_provider.account_code(account)?
        .ok_or_eyre("No code found for address")?;
    let code_bytes = code.original_byte_slice().to_vec();
    Ok(code_bytes)
}


fn hex_or_vec_opt<'de, D>(deserializer: D) -> Result<Option<Vec<u8>>, D::Error>
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
