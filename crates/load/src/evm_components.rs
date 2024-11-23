use revm::{
    handler::register::EvmHandler,
    primitives::B256,
    Database,
};
pub use libloading::Library;
pub use revmc::EvmCompilerFn;

use rustc_hash::FxHashMap;
use std::sync::Arc;
use revm::primitives::Address;
use revmc_toolkit_build::{JitCompileOut, JitCompileCtx};

#[derive(Default, Clone)]
pub struct EvmCompilerFns(pub Arc<FxHashMap<B256, (EvmCompilerFn, ReferenceDropObject)>>);

impl EvmCompilerFns {
    pub fn get(&self, bytecode_hash: &B256) -> Option<&(EvmCompilerFn, ReferenceDropObject)> {
        self.0.get(bytecode_hash)
    }
}

impl From<Vec<(B256, EvmCompilerFn)>> for EvmCompilerFns {
    fn from(fns: Vec<(B256, EvmCompilerFn)>) -> Self {
        let compiled_fns = fns.into_iter()
            .map(|(h, f)| (h, (f, ReferenceDropObject::None)))
            .collect();
        Self(Arc::new(compiled_fns))
    }
}

impl From<Vec<(B256, (EvmCompilerFn, Library))>> for EvmCompilerFns {
    fn from(fns: Vec<(B256, (EvmCompilerFn, Library))>) -> Self {
        let compiled_fns = fns.into_iter()
            .map(|(h, (fnc, lib))| (h, (fnc, ReferenceDropObject::Library(lib))))
            .collect();
        Self(Arc::new(compiled_fns))
    }
}

impl From<JitCompileOut> for EvmCompilerFns {
    fn from(JitCompileOut { entries, ctx }: JitCompileOut) -> Self {
        let compiler_ctx = Arc::new(ctx);
        let compiled_fns = entries.into_iter()
            .map(|(h, fnc)| (h, (fnc, ReferenceDropObject::CompilerCtx(compiler_ctx.clone()))))
            .collect();
        Self(Arc::new(compiled_fns))
    }
}

#[derive(Default, Clone)]
pub struct RevmcExtCtx {
    compiled_fns: EvmCompilerFns,
    pub touches: Option<Touches>
}

impl RevmcExtCtx {

    pub fn with_touch_tracking(mut self) -> Self {
        self.touches = Some(Touches::default());
        self
    }

}

impl From<Vec<(B256, EvmCompilerFn)>> for RevmcExtCtx {
    fn from(fns: Vec<(B256, EvmCompilerFn)>) -> Self {
        Self { compiled_fns: fns.into(), touches: None }
    }
}

impl From<Vec<(B256, (EvmCompilerFn, Library))>> for RevmcExtCtx {
    fn from(fns: Vec<(B256, (EvmCompilerFn, Library))>) -> Self {
        Self { compiled_fns: fns.into(), touches: None }
    }
}

impl From<EvmCompilerFns> for RevmcExtCtx {
    fn from(fns: EvmCompilerFns) -> Self {
        Self { compiled_fns: fns, touches: None }
    }
}

pub trait RevmcExtCtxExtTrait {
    fn get_function(&self, bytecode_hash: B256) -> Option<EvmCompilerFn>;
    fn register_touch(&mut self, address: Address, non_native: bool);
    fn touches(&self) -> Option<&Touches>;
}

impl RevmcExtCtxExtTrait for RevmcExtCtx {
    fn get_function(&self, bytecode_hash: B256) -> Option<EvmCompilerFn> {
        self.compiled_fns.get(&bytecode_hash).map(|f| f.0)
    }
    fn register_touch(&mut self, address: Address, non_native: bool) {
        self.touches.as_mut().map(|t| t.register_touch(address, non_native));
    }
    fn touches(&self) -> Option<&Touches> {
        self.touches.as_ref()
    }
}

pub enum ReferenceDropObject {
    #[allow(dead_code)]
    Library(Library),
    CompilerCtx(Arc<JitCompileCtx>),
    None,
}

pub fn revmc_register_handler<DB, ExtCtx>(handler: &mut EvmHandler<'_, ExtCtx, DB>) 
    where DB: Database, ExtCtx: RevmcExtCtxExtTrait
{    
    let execute_frame_original = handler.execution.execute_frame.clone();
    handler.execution.execute_frame = Arc::new(move |frame, memory, tables, context| {
        let interpreter = frame.interpreter_mut();
        let bytecode_hash = interpreter.contract.hash.unwrap_or_default();
        let ext_fn = context.external.get_function(bytecode_hash);

        // todo: check how much overhead could this conditional add
        context.external.register_touch(
            interpreter.contract.bytecode_address
                .unwrap_or(interpreter.contract.target_address), 
            ext_fn.is_some()
        );

        Ok(if let Some(f) = ext_fn {
            unsafe { f.call_with_interpreter_and_memory(interpreter, memory, context) }
        } else {
            execute_frame_original(frame, memory, tables, context)?
        })
    });
}

#[derive(Default, Debug, Clone)]
pub struct Touches(FxHashMap<Address, TouchCounter>);

impl Touches {

    pub fn inner(&self) -> &FxHashMap<Address, TouchCounter> {
        &self.0
    }

    pub fn into_inner(self) -> FxHashMap<Address, TouchCounter> {
        self.0
    }
    
    fn register_touch(&mut self, address: Address, non_native: bool) {
        self.0.entry(address)
            .and_modify(|c| c.increment(non_native))
            .or_insert(TouchCounter::new_with_increment(non_native));
    }
}

#[derive(Default, Debug, Clone)]
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