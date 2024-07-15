use revm::{
    primitives::B256,
    handler::register::EvmHandler,
    Database,
};
use libloading::Library;
use revmc::{EvmCompiler, EvmLlvmBackend, EvmCompilerFn};

use std::collections::HashMap;
use std::sync::Arc;
use eyre::Result;


// todo: rename as it is not only aot

enum ReferenceDropObject {
    Library(Library),
    EvmCompilerFn(EvmCompiler<EvmLlvmBackend<'static>>),
    None,
}


pub fn build_external_context(
    dir_path: &str, 
    codehash_select: Option<Vec<B256>>
) -> Result<ExternalContext> {
    let loader = crate::fn_loader::EvmCompilerFnLoader::new(dir_path);
    let fncs = match codehash_select {
        Some(codehash_select) => loader.load_selected(codehash_select)?,
        None => loader.load_all()?,
    };
    let external_ctx = ExternalContext(
        fncs.into_iter().map(|(h, (fnc, lib))| (h, (fnc, ReferenceDropObject::Library(lib)))).collect()
    );

    Ok(external_ctx)
}

#[derive(Default)]
pub struct ExternalContext(HashMap<B256, (EvmCompilerFn, ReferenceDropObject)>);  // todo: consider fast hashmap

impl ExternalContext {

    pub fn from_fns(fns: Vec<(B256, EvmCompilerFn)>) -> Self {
        Self(fns.into_iter().map(|(h, f)| (h, (f, ReferenceDropObject::None))).collect())
    }

    pub fn add(&mut self, code_hash: B256, fnc: (EvmCompilerFn, ReferenceDropObject)) {
        self.0.insert(code_hash, fnc);
    }

    fn get_function(&self, bytecode_hash: B256) -> Option<EvmCompilerFn> {
        self.0.get(&bytecode_hash).map(|f| f.0)
    }
}

// todo: add touches and cummulative gas per contract

pub fn register_handler<DB: Database>(handler: &mut EvmHandler<'_, ExternalContext, DB>) {
    let prev = handler.execution.execute_frame.clone();
    handler.execution.execute_frame = Arc::new(move |frame, memory, tables, context| {
        let interpreter = frame.interpreter_mut();
        let bytecode_hash = interpreter.contract.hash.unwrap_or_default();

        let res = if let Some(f) = context.external.get_function(bytecode_hash) {
            unsafe { f.call_with_interpreter_and_memory(interpreter, memory, context) }
        } else {
            prev(frame, memory, tables, context)?
        };
        // res.clone().into_result_return()?.gas - cummulative gas
        Ok(res)
    });
}
