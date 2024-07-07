use revm::{
    primitives::{FixedBytes, EnvWithHandlerCfg},
    handler::register::EvmHandler,
    db::CacheDB,
    Database,
    Evm, 
};
use reth_primitives::B256;
use libloading::Library;
use revmc::EvmCompilerFn;

use std::collections::HashMap;
use std::sync::Arc;
use eyre::Result;

use crate::fn_loader;


pub struct SourceCode {
    pub label: String,
    pub codehash: FixedBytes<32>,
}

impl From<(String, FixedBytes<32>)> for SourceCode {
    fn from((label, codehash): (String, FixedBytes<32>)) -> Self {
        Self { label, codehash }
    }
}

// todo: this should only be an example I think
// todo: would it better external ctx is created once? and all the fns loaded once?
pub fn create_evm<ExtDB: revm::Database + revm::DatabaseRef>(
    dir_path: String,
    bytecode_hashes: Option<Vec<B256>>, 
    db: CacheDB<ExtDB>, 
    cfg_env: EnvWithHandlerCfg
) -> Result<Evm<'static, ExternalContext, CacheDB<ExtDB>>> {
    let external_ctx = build_ext_ctx(dir_path, bytecode_hashes)?;
    let evm = revm::Evm::builder()
        .with_db(db)
        .with_external_context(external_ctx)
        .with_env_with_handler_cfg(cfg_env)
        .append_handler_register(register_handler)
        .build();
    Ok(evm)
}

pub fn build_ext_ctx(dir_path: String, bytecode_hashes: Option<Vec<B256>>) -> Result<ExternalContext> {
    println!("building ext ctx");
    let loader = fn_loader::EvmCompilerFnLoader::new(dir_path);
    let fncs = match bytecode_hashes {
        Some(bytecode_hashes) => loader.load_selected(bytecode_hashes)?,
        None => loader.load_all()?,
    };
    let external_ctx = ExternalContext(fncs.into_iter().collect());

    Ok(external_ctx)
}

#[derive(Default)]
pub struct ExternalContext(HashMap<B256, (EvmCompilerFn, Library)>);  // todo: consider fast hashmap

impl ExternalContext {
    fn add(&mut self, code_hash: B256, fnc: (EvmCompilerFn, Library)) {
        self.0.insert(code_hash, fnc);
    }

    fn get_function(&self, bytecode_hash: B256) -> Option<EvmCompilerFn> {
        self.0.get(&bytecode_hash).map(|f| f.0)
    }
}

pub fn register_handler<DB: Database>(handler: &mut EvmHandler<'_, ExternalContext, DB>) {
    let prev = handler.execution.execute_frame.clone();
    handler.execution.execute_frame = Arc::new(move |frame, memory, tables, context| {
        let interpreter = frame.interpreter_mut();
        let bytecode_hash = interpreter.contract.hash.unwrap_or_default();
        if let Some(f) = context.external.get_function(bytecode_hash) {
            Ok(unsafe { f.call_with_interpreter_and_memory(interpreter, memory, context) })
        } else {
            prev(frame, memory, tables, context)
        }
    });
}
