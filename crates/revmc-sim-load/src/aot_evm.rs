use revm::{
    primitives::{B256, Address},
    handler::register::EvmHandler,
    Database,
};
use libloading::Library;
use revmc::EvmCompilerFn;

use std::collections::HashMap;
use std::sync::Arc;
use eyre::Result;


// todo: rename as it is not only aot

pub fn build_external_context(
    dir_path: &str, 
    codehash_select: Option<Vec<B256>>
) -> Result<ExternalContext> {
    let loader = crate::fn_loader::EvmCompilerFnLoader::new(dir_path);
    let fncs = match codehash_select {
        Some(codehash_select) => loader.load_selected(codehash_select)?,
        None => loader.load_all()?,
    };
    Ok(fncs.into())
}

#[derive(Default)]
pub struct ExternalContext {
    compiled_fns: HashMap<B256, (EvmCompilerFn, ReferenceDropObject)>,
    pub touches: Option<HashMap<Address, TouchCounter>>,
}  // todo: consider fast hashmap

impl ExternalContext {

    fn get_function(&self, bytecode_hash: B256) -> Option<EvmCompilerFn> {
        self.compiled_fns.get(&bytecode_hash).map(|f| f.0)
    }

    fn register_touch(&mut self, address: Address, non_native: bool) {
        let touches = self.touches.get_or_insert_with(HashMap::new);
        touches.entry(address)
            .and_modify(|c| c.increment(non_native))
            .or_insert(TouchCounter::new_with_increment(non_native));
    }
}

impl From<Vec<(B256, EvmCompilerFn)>> for ExternalContext {
    fn from(fns: Vec<(B256, EvmCompilerFn)>) -> Self {
        let compiled_fns = fns.into_iter()
            .map(|(h, f)| (h, (f, ReferenceDropObject::None)))
            .collect();
        Self { compiled_fns, touches: None }
    }
}

impl From<Vec<(B256, (EvmCompilerFn, Library))>> for ExternalContext {
    fn from(fns: Vec<(B256, (EvmCompilerFn, Library))>) -> Self {
        let compiled_fns = fns.into_iter()
            .map(|(h, (fnc, lib))| (h, (fnc, ReferenceDropObject::Library(lib))))
            .collect();
        Self { compiled_fns, touches: None }
    }
}

// todo: track gas consumption
#[derive(Default, Debug)]
pub struct TouchCounter {
    pub overall: usize,
    pub non_native: usize,
}

impl TouchCounter {

    fn new_with_increment(non_native: bool) -> Self {
        let mut counter = Self::default();
        counter.increment(non_native);
        counter
    }

    fn increment(&mut self, non_native: bool) {
        self.overall += 1;
        if non_native {
            self.non_native += 1;
        }
    }
}

enum ReferenceDropObject {
    #[allow(dead_code)]
    Library(Library),
    None,
}

pub fn register_handler<DB: Database>(handler: &mut EvmHandler<'_, ExternalContext, DB>) {
    let prev = handler.execution.execute_frame.clone();
    handler.execution.execute_frame = Arc::new(move |frame, memory, tables, context| {
        let interpreter = frame.interpreter_mut();
        let bytecode_hash = interpreter.contract.hash.unwrap_or_default();
        let ext_fn = context.external.get_function(bytecode_hash);

        context.external.register_touch(
            interpreter.contract.target_address, 
            ext_fn.is_some()
        );

        Ok(if let Some(f) = ext_fn {
            unsafe { f.call_with_interpreter_and_memory(interpreter, memory, context) }
        } else {
            prev(frame, memory, tables, context)?
        })
    });
}
