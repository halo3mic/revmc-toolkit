use revmc::{
    llvm::inkwell::context::Context,
    EvmLlvmBackend, 
    EvmCompiler, 
};
use revm::primitives::SpecId;

use eyre::{ensure, Ok, Result};
use serde::Deserialize;
use std::path::PathBuf;

use crate::utils::{self, OptimizationLevelDeseralizable};


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
    pub out_dir: Option<PathBuf>,
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

impl CompilerOptions {
    fn new() -> Self {
        Self {
            out_dir: None,
            target: "native".to_string(),
            target_cpu: None,
            target_features: None,
            no_gas: true, // todo try true for performance
            no_len_checks: true, // todo try true for performance
            frame_pointers: false,
            debug_assertions: false,
            no_link: false,
            opt_level: OptimizationLevelDeseralizable::Aggressive, // todo: try aggresive
            spec_id: SpecId::CANCUN, // ! EOF yet not implemented
            label: None,
        }
    }
}

impl CompilerOptions {
    pub fn with_label(mut self, label: impl ToString) -> Self {
        self.label = Some(label.to_string());
        self
    }
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl Into<AOTCompiler> for CompilerOptions {
    fn into(self) -> AOTCompiler {
        AOTCompiler { opt: self }
    }
}

#[derive(Default)]
pub struct AOTCompiler {
    opt: CompilerOptions,
}

impl AOTCompiler {
    
    pub fn compile(&self, bytecode: &[u8]) -> Result<()> {    
        let name = utils::bytecode_hash_str(bytecode);

        let ctx = Context::create();
        let mut compiler = self.create_compiler(&ctx, &name)?;    
        compiler.translate(Some(&name), bytecode, self.opt.spec_id)?;

        let out_dir = self.out_dir(&name)?;
        let obj = Self::write_precompiled_obj(&mut compiler, &name, &out_dir)?;
        if !self.opt.no_link {
            Self::link(&obj, &out_dir)?;
        }
    
        // todo: if label exists link it to the bytecode hash
        Ok(())
    }
    
    fn create_compiler<'a>(&self, ctx: &'a Context, name: &str) -> Result<EvmCompiler<EvmLlvmBackend<'a>>> {
        let target = self.create_target();
        let backend = EvmLlvmBackend::new_for_target(
            ctx, true, self.opt.opt_level.clone().into(), &target
        )?;
        let mut compiler = EvmCompiler::new(backend);
    
        compiler.set_dump_to(self.opt.out_dir.clone());
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
        println!("Writing object file to {}", obj.display());
        compiler.write_object_to_file(&obj)?;
        if !obj.exists() {
            return Err(eyre::eyre!("Failed to compile object file"));
        }
        Ok(obj)
    }
    
    fn link(obj: &PathBuf, out_dir: &PathBuf) -> Result<()> {
        let so = out_dir.join("a.so");
        revmc::Linker::new()
            .link(&so, [obj.to_str().unwrap()])?;
        ensure!(so.exists(), "Failed to link object file");
        eprintln!("Linked shared object file to {}", so.display());
        Ok(())
    }

    fn out_dir(&self, name: &str) -> Result<PathBuf> {
        let out_dir = match &self.opt.out_dir {
            Some(dir) => dir,
            None => &utils::default_dir(),
        };
        let out_dir = out_dir.join(name);
        utils::make_dir(&out_dir)?;
        Ok(out_dir.to_path_buf())
    }

}