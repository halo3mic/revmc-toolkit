mod utils;
mod compiler;

use serde::{Deserialize, Deserializer};
use std::path::PathBuf;
use serde_json::Value;
use rayon::prelude::*;
use eyre::Result;
use hex;

pub use compiler::{CompilerOptions, AOTCompiler};


// todo: is this still relevant given we have similar functionality in cli? + address field is a problem
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
) -> Result<Vec<Result<()>>> {
    // todo: check for duplicates among the args
    Ok(args.into_par_iter()
        .map(|arg| compile_contract(&arg.code, arg.options.or(fallback_opt.clone())))
        .collect())
}

pub fn compile_contract(code: &[u8], options: Option<CompilerOptions>) -> Result<()> {
    let compiled_contracts = load_compiled(utils::default_dir())?; // todo: does it make sense to load this every time?
    let is_compiled = compiled_contracts.contains(&utils::bytecode_hash_str(&code));
    if !is_compiled {
        let compiler: AOTCompiler = options.unwrap_or_default().into();
        return compiler.compile(&code);
    }
    return Ok(());
}

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

fn load_compiled(path: PathBuf) -> Result<Vec<String>> {
    let vec = std::fs::read_dir(path)?
        .map(|res| res.map(|e| {
            let path = e.path();
            let file_name = path.file_name()
                .unwrap().to_owned().into_string().unwrap();
            file_name
        }))
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    Ok(vec)
}