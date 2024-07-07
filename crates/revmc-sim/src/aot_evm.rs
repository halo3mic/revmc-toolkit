use revm::{
    primitives::FixedBytes,
    handler::register::EvmHandler,
    EvmBuilder,
    Database,
    Evm, 
};
use reth_primitives::B256;
use libloading::Library;
use reth_provider::StateProvider;
use reth_revm::database::StateProviderDatabase;
use revm::{
    db::CacheDB
};


use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use eyre::Result;
use std::path::Path;

use crate::build;


struct EvmCompilerFnStore {
    dir_path: String
}

impl EvmCompilerFnStore {
    fn new(dir_path: String) -> Self {
        Self { dir_path }
    }

    // load all
    // load single(label)
    // load config

    fn load(&self, label: &str) -> Result<(revmc::EvmCompilerFn, Library)> {
        let fnc = build::load(label)?;
        Ok(fnc)
    }
}




pub struct SourceCode {
    pub label: String,
    pub codehash: FixedBytes<32>,
}

impl From<(String, FixedBytes<32>)> for SourceCode {
    fn from((label, codehash): (String, FixedBytes<32>)) -> Self {
        Self { label, codehash }
    }
}

pub fn create_evm<E>(
    src_codes: Vec<SourceCode>, 
    db: impl Database<Error=E> + 'static
) -> Result<Evm<'static, ExternalContext, impl Database<Error=E>>> 
where E: std::fmt::Debug + 'static
{
    let external_ctx = build_ext_ctx(src_codes)?;
    let evm = revm::Evm::builder()
        .with_db(db)
        .with_external_context(external_ctx)
        .append_handler_register(register_handler)
        .build();
    Ok(evm)
}

// fn build_partial_evm<DB: Database>(src_codes: Vec<SourceCode>) -> Result<EvmBuilder<'static, _, ExternalContext, DB>> {
//     let external_ctx = build_ext_ctx(src_codes)?;
//     let evm_builder = revm::Evm::builder()
//         .with_external_context(external_ctx)
//         .append_handler_register(register_handler);
//     Ok(evm_builder)
// }

pub fn build_ext_ctx(src_codes: Vec<SourceCode>) -> Result<ExternalContext> {
    let mut external_ctx = ExternalContext::default();
    for src_code in src_codes {
        let fnc  = build::load(&src_code.label)?; // todo: does it make sense to define load fn here
        external_ctx.add(src_code.codehash, fnc);
    }
    Ok(external_ctx)
}

#[derive(Default)]
pub struct ExternalContext(HashMap<B256, (revmc::EvmCompilerFn, Library)>);  // todo: consider fast hashmap

impl ExternalContext {
    fn add(&mut self, code_hash: B256, fnc: (revmc::EvmCompilerFn, Library)) {
        self.0.insert(code_hash, fnc);
    }

    fn get_function(&self, bytecode_hash: B256) -> Option<revmc::EvmCompilerFn> {
        self.0.get(&bytecode_hash).map(|f| f.0)
    }
}

// This `+ 'static` bound is only necessary here because of an internal cfg feature.
pub fn register_handler<DB: Database + 'static>(handler: &mut EvmHandler<'_, ExternalContext, DB>) {
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
