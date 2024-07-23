mod utils;
mod compiler;

use serde::{Deserialize, Deserializer};
use std::path::PathBuf;
use serde_json::Value;
use rayon::prelude::*;
use eyre::Result;
use hex;
use tracing::debug;

pub use compiler::{CompilerOptions, Compiler, JitCompileOut};
pub use utils::default_dir;


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

pub fn compile_contracts_aot(
    args: Vec<CodeWithOptions>, 
    fallback_opt: Option<CompilerOptions>
) -> Result<Vec<Result<()>>> {
    // todo: check for duplicates among the args
    Ok(args.into_par_iter()
        .map(|arg| compile_contract_aot(&arg.code, arg.options.or(fallback_opt.clone())))
        .collect())
}

pub fn compile_contracts_jit(
    args: Vec<CodeWithOptions>, 
    fallback_opt: Option<CompilerOptions>
) -> Result<Vec<Result<JitCompileOut>>> {
    // todo: check for duplicates among the args
    Ok(args.into_par_iter()
        .map(|arg| compile_contract_jit(&arg.code, arg.options.or(fallback_opt.clone())))
        .collect())
}

pub fn compile_contract_aot(code: &[u8], options: Option<CompilerOptions>) -> Result<()> {
    // todo: does it make sense to load this every time? - instead just do it once? loader struct?
    let compiled_contracts = load_compiled(utils::default_dir()).unwrap_or_default(); 
    let is_compiled = compiled_contracts.contains(&utils::bytecode_hash_str(&code));
    debug!("Compiling AOT contract; is compiled: {is_compiled}");
    if !is_compiled {
        let compiler: Compiler = options.unwrap_or_default().into();
        return compiler.compile_aot(&code);
    }
    return Ok(());
}

pub fn compile_contract_jit(code: &[u8], options: Option<CompilerOptions>) -> Result<JitCompileOut> {
    let compiler: Compiler = options.unwrap_or_default().into();
    return compiler.compile_jit(&code);
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