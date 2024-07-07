use revmc::{
    llvm::inkwell::context::Context,
    OptimizationLevel,
    EvmLlvmBackend, 
    EvmCompiler, 
};
use reth_primitives::Bytecode;
use revm::primitives::SpecId;
use libloading::Library;

use eyre::{ensure, Ok, Result};
use std::path::PathBuf;
use rayon::prelude::*;

// todo: it would make sense to store also code hashes
// todo: offer option to load all contracts from a dir

const DEFAULT_DATA_DIR: &str = ".data";

pub struct CompileArgs {
    pub label: String,
    pub code: Bytecode,
    pub options: Option<CompilerOptions>,
}

pub fn compile_contracts(args: Vec<CompileArgs>) -> Vec<Result<()>> {
    args.into_par_iter()
        .map(|arg| compile_contract(arg))
        .collect()
}

pub fn compile_contract(arg: CompileArgs) -> Result<()> {
    compile(&arg.label, &arg.code, arg.options)
}

/**
 * Performance considerations:
 * - Disabled gas metering can improve performance, but it could result in an infinite loop.
 * - Without length checks performance may be improved, but it could result in undefined behaviour if stack overflows.
 * - Frame pointers are useful for debugging, but they can be disabled to slightly improve performance.
 * - Useful for debugging, but it can be disabled for moderate performance improvement.
 */
pub struct CompilerOptions {
    out_dir: Option<PathBuf>,
    spec_id: SpecId,

    target_features: Option<String>,
    target_cpu: Option<String>,
    target: String,

    opt_level: OptimizationLevel,
    no_link: bool,
    no_gas: bool,    
    no_len_checks: bool,
    frame_pointers: bool,
    debug_assertions: bool,
}

impl CompilerOptions {
    fn new() -> Self {
        Self {
            out_dir: None,
            target: "native".to_string(),
            target_cpu: None,
            target_features: None,
            no_gas: false, // todo try true for performance
            no_len_checks: false, // todo try true for performance
            frame_pointers: false,
            debug_assertions: false,
            no_link: false,
            opt_level: OptimizationLevel::Default, // todo: try aggresive
            spec_id: SpecId::CANCUN, // ! EOF yet not implemented
        }
    }
}

impl Default for CompilerOptions {
    fn default() -> Self {
        Self::new()
    }
}

fn compile(label: &str, bytecode: &Bytecode, opt: Option<CompilerOptions>) -> Result<()> {
    let opt = opt.unwrap_or_default();
    let ctx = Context::create();
    let mut compiler = create_compiler(label, &ctx, &opt)?;

    let CompilerOptions { out_dir, no_link, spec_id, .. } = opt; 
    let out_dir = out_dir.map_or_else(
        || create_default_dir(label),
        |dir| Ok(dir)
    )?;
    
    compiler.translate(Some(&label), bytecode.bytes_slice(), spec_id)?;
    let obj = write_precompiled_obj(&mut compiler, label, &out_dir)?;

    if !no_link {
        link(&obj, &out_dir)?;
    }

    Ok(())
}

fn create_compiler<'a>(
    label: &str, 
    ctx: &'a Context, 
    opt: &CompilerOptions
) -> Result<EvmCompiler<EvmLlvmBackend<'a>>> {
    let target = revmc::Target::new(&opt.target, opt.target_cpu.clone(), opt.target_features.clone());
    let backend = EvmLlvmBackend::new_for_target(ctx, true, opt.opt_level, &target)?;
    let mut compiler = EvmCompiler::new(backend);

    compiler.set_dump_to(opt.out_dir.clone());
    compiler.gas_metering(!opt.no_gas);
    unsafe { compiler.stack_bound_checks(!opt.no_len_checks) };
    compiler.frame_pointers(opt.frame_pointers);
    compiler.debug_assertions(opt.debug_assertions);
    compiler.inspect_stack_length(true);
    compiler.set_module_name(label);

    Ok(compiler)
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
    let linker = revmc::Linker::new();
    linker.link(&so, [obj.to_str().unwrap()])?;
    ensure!(so.exists(), "Failed to link object file");
    eprintln!("Linked shared object file to {}", so.display());
    Ok(())
}

// todo: define path
pub fn load(label: &str) -> Result<(revmc::EvmCompilerFn, Library)> {
    let path = default_dir(label).join("a.so");
    println!("Loading {label} at path {}", path.display());
    let lib = unsafe { Library::new(path) }?;
    let f: libloading::Symbol<'_, revmc::EvmCompilerFn> =
        unsafe { lib.get(label.as_bytes())? };
    Ok((*f, lib))
}

fn default_dir(label: &str) -> PathBuf {
    std::env::current_dir()
        .expect("Failed to get current directory")
        .join(DEFAULT_DATA_DIR)
        .join(label)
}

fn create_default_dir(label: &str) -> Result<PathBuf> {
    let dir = default_dir(label);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}