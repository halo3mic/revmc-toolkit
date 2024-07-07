mod utils;
mod compiler;

use std::path::PathBuf;
use rayon::prelude::*;
use eyre::Result;

pub use compiler::CompilerOptions;


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


#[derive(serde::Deserialize, Debug, Clone)]
pub struct CodeWithOptions {
    #[serde(deserialize_with = "hex_or_vec")]
    pub code: Vec<u8>,
    pub options: Option<CompilerOptions>,
}

impl From<Vec<u8>> for CodeWithOptions {
    fn from(code: Vec<u8>) -> Self {
        Self { code, options: None }
    }
}

pub fn compile_contracts(
    args: Vec<CodeWithOptions>, 
    fallback_opt: Option<CompilerOptions>
) -> Vec<Result<()>> {
    args.into_par_iter()
        .map(|arg| compile_contract(arg.code, arg.options.or(fallback_opt.clone())))
        .collect()
}

pub fn compile_contract(code: Vec<u8>, options: Option<CompilerOptions>) -> Result<()> {
    let compiler: compiler::AOTCompiler = options.unwrap_or_default().into();
    compiler.compile(&code)
}

use serde::{Deserialize, Deserializer};
use serde_json::Value;
use hex;

fn hex_or_vec<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    
    match value {
        Value::String(s) => {
            let s = s.trim_start_matches("0x");
            hex::decode(s).map_err(serde::de::Error::custom)
        },
        Value::Array(arr) => arr
            .into_iter()
            .map(|v| v.as_u64().ok_or_else(|| serde::de::Error::custom("Expected a number"))
                .and_then(|n| u8::try_from(n).map_err(serde::de::Error::custom)))
            .collect(),
        _ => Err(serde::de::Error::custom("Expected hex string or array of numbers")),
    }
}