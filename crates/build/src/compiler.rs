use revmc::{
    llvm::inkwell::context::Context,
    EvmLlvmBackend, 
    EvmCompiler,
    EvmCompilerFn,
};
use revm::primitives::{SpecId, B256};

use eyre::{OptionExt, Result};
use serde::Deserialize;
use std::path::PathBuf;
use tracing::debug;

use crate::utils::{self, OptimizationLevelDeseralizable};


pub type JitCompileOut = (B256, EvmCompilerFn);

/**
 * Performance considerations:
 * - Disabled gas metering can improve performance, but it could result in an infinite loop.
 * - Without length checks performance may be improved, but it could result in undefined behaviour if stack overflows.
 * - Frame pointers are useful for debugging, but they can be disabled to slightly improve performance.
 * - Useful for debugging, but it can be disabled for moderate performance improvement.
 */
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompilerOptions {
    pub out_dir: PathBuf,
    pub spec_id: SpecId,

    pub target_features: Option<String>,
    pub target_cpu: Option<String>,
    pub target: String,

    pub opt_level: OptimizationLevelDeseralizable,
    pub no_link: bool,
    pub no_gas: bool,    
    pub no_len_checks: bool,
    pub frame_pointers: bool,
    pub debug_assertions: bool,
    pub label: Option<String>,
}

// todo: add setters
impl CompilerOptions {
    pub fn with_label(mut self, label: impl ToString) -> Self {
        self.label = Some(label.to_string());
        self
    }
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self {
            out_dir: utils::default_dir(),
            target: "native".to_string(),
            target_cpu: None,
            target_features: None,
            no_gas: false,
            no_len_checks: false,
            frame_pointers: false,
            debug_assertions: false,
            no_link: false,
            opt_level: OptimizationLevelDeseralizable::Aggressive,
            spec_id: SpecId::CANCUN,
            label: None,
        }
    }
}

impl Into<Compiler> for CompilerOptions {
    fn into(self) -> Compiler {
        Compiler { opt: self }
    }
}

#[derive(Default)]
pub struct Compiler {
    opt: CompilerOptions,
}

impl Compiler {
    
    pub fn compile_aot(&self, bytecode: &[u8]) -> Result<()> {  
        let name = utils::bytecode_hash_str(bytecode);
        debug!("Compiling AOT contract with name {}", name);  

        let ctx: &'static Context = Box::leak(Box::new(Context::create()));
        let mut compiler = self.create_compiler(ctx, &name, true)?;    
        compiler.translate(&name, bytecode, self.opt.spec_id)?;

        let out_dir = self.out_dir(&name)?;
        let obj = Self::write_precompiled_obj(&mut compiler, &name, &out_dir)?;
        if !self.opt.no_link {
            Self::link(&obj, &out_dir)?;
        }
        Ok(())
    }

    pub fn compile_jit(&self, bytecode: &[u8]) -> Result<JitCompileOut> {
        self.compile_jit_many(vec![bytecode]).map(|mut v| v.pop().unwrap())
    }

    // ! LEAKING MEMORY - only for demo to avoid segmentation fault
    // todo: return a struct that when dropped also drops the context and compiler
    pub fn compile_jit_many(&self, bytecodes: Vec<&[u8]>) -> Result<Vec<JitCompileOut>> {
        let ctx: &'static Context = Box::leak(Box::new(Context::create()));
        let mut compiler = self.create_compiler(ctx, "compile_many", false)?;

        // First we translate all at once, only then we finalize them
        let fn_ids = bytecodes.into_iter().map(|bytecode| {
            let bytecode_hash = revm::primitives::keccak256(bytecode);
            let name = bytecode_hash.to_string();
            debug!("Compiling JIT contract with name {}", name);
            let fn_id = compiler.translate(&name, bytecode, self.opt.spec_id)?;
            Ok((bytecode_hash, fn_id))
        }).collect::<Result<Vec<_>>>()?;  
        let fncs = fn_ids.into_iter().map(|(bytecode_hash, fn_id)| {
            let fnc = unsafe { compiler.jit_function(fn_id)? };
            Ok((bytecode_hash, fnc))
        }).collect::<Result<Vec<_>>>()?;
        Box::leak(Box::new(compiler));

        Ok(fncs)
    }
    
    fn create_compiler(&self, ctx: &'static Context, name: &str, aot: bool) -> Result<EvmCompiler<EvmLlvmBackend<'static>>> {
        let target = self.create_target();
        let backend = EvmLlvmBackend::new_for_target(
            ctx, aot, self.opt.opt_level.clone().into(), &target
        )?;
        let mut compiler = EvmCompiler::new(backend);
    
        compiler.set_dump_to(Some(self.opt.out_dir.clone()));
        compiler.gas_metering(!self.opt.no_gas);
        unsafe { compiler.stack_bound_checks(!self.opt.no_len_checks) };
        compiler.frame_pointers(self.opt.frame_pointers);
        compiler.debug_assertions(self.opt.debug_assertions);
        compiler.inspect_stack_length(true);
        compiler.set_module_name(name);
    
        Ok(compiler)
    }

    fn create_target(&self) -> revmc::Target {
        revmc::Target::new(
            &self.opt.target, 
            self.opt.target_cpu.clone(), 
            self.opt.target_features.clone()
        )
    }
    
    fn write_precompiled_obj(
        compiler: &mut EvmCompiler<EvmLlvmBackend>,
        label: &str,
        out_dir: &PathBuf,
    ) -> Result<PathBuf> {
        let obj = out_dir.join(label).with_extension("o");
        debug!("Writing object file to {}", obj.display());
        compiler.write_object_to_file(&obj)?;
        if !obj.exists() {
            return Err(eyre::eyre!("Failed to compile object file"));
        }
        Ok(obj)
    }
    
    fn link(obj: &PathBuf, out_dir: &PathBuf) -> Result<()> {
        let so = out_dir.join("a.so");
        let obj_str = obj.to_str().ok_or_eyre("Invalid object file path")?;
    
        for _ in 0..10 {
            revmc::Linker::new().link(&so, [obj_str])?;
            if so.exists() {
                debug!("Linked shared object file to {}", so.display());
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        Err(eyre::eyre!("Failed to link object file after 10 attempts"))
    }

    fn out_dir(&self, name: &str) -> Result<PathBuf> {
        let out_dir = self.opt.out_dir.join(name);
        utils::make_dir(&out_dir)?;
        Ok(out_dir.to_path_buf())
    }

}