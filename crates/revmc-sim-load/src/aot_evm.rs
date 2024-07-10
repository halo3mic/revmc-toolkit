use revm::{
    primitives::B256,
    handler::register::EvmHandler,
    Database,
};
use libloading::Library;
use revmc::EvmCompilerFn;

use std::collections::HashMap;
use std::sync::Arc;
use eyre::Result;


pub fn build_external_context(
    dir_path: String, 
    codehash_select: Option<Vec<B256>>
) -> Result<ExternalContext> {
    let loader = crate::fn_loader::EvmCompilerFnLoader::new(dir_path);
    let fncs = match codehash_select {
        Some(codehash_select) => loader.load_selected(codehash_select)?,
        None => loader.load_all()?,
    };
    let external_ctx = ExternalContext(fncs.into_iter().collect());

    Ok(external_ctx)
}

#[derive(Default)]
pub struct ExternalContext(HashMap<B256, (EvmCompilerFn, Library)>);  // todo: consider fast hashmap

impl ExternalContext {
    pub fn add(&mut self, code_hash: B256, fnc: (EvmCompilerFn, Library)) {
        self.0.insert(code_hash, fnc);
    }

    fn get_function(&self, bytecode_hash: B256) -> Option<EvmCompilerFn> {
        self.0.get(&bytecode_hash).map(|f| f.0)
    }
}

// todo: rm 
// use std::time::Instant;

pub fn register_handler<DB: Database>(handler: &mut EvmHandler<'_, ExternalContext, DB>) {
    let prev = handler.execution.execute_frame.clone();
    handler.execution.execute_frame = Arc::new(move |frame, memory, tables, context| {
        let interpreter = frame.interpreter_mut();
        let bytecode_hash = interpreter.contract.hash.unwrap_or_default();
        // println!("Calling fn on {:?} with codehash {bytecode_hash:?}", interpreter.contract.target_address);
        if let Some(f) = context.external.get_function(bytecode_hash) {
            // println!("Calling AOT fn with input {:?}", interpreter.contract.input);
            // let start = Instant::now();
            let res = unsafe { f.call_with_interpreter_and_memory(interpreter, memory, context) };
            // let elapsed = start.elapsed();
            // println!("AOT fn took: {:?}", elapsed);
            Ok(res)
        } else {
            // let start = Instant::now();
            let res = prev(frame, memory, tables, context);
            // let elapsed = start.elapsed();
            // println!("Native fn took: {:?}", elapsed);
            res
        }
    });
}
