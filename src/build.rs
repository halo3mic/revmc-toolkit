use revmc::{EvmLlvmBackend, EvmCompiler, OptimizationLevel};
use reth_primitives::Bytecode;
use revm::primitives::SpecId;

use std::path::PathBuf;
use eyre::{Result, ensure};


pub struct CompilerOptions {
    out_dir: Option<PathBuf>,
    spec_id: SpecId,

    target: String,
    target_cpu: Option<String>,
    target_features: Option<String>,

    opt_level: OptimizationLevel,
    no_link: bool,
    // Disabled gas metering can improve performance, but it could result in an infinite loop.
    no_gas: bool,    
    // Without length checks performance may be improved, but it could result in undefined behaviour if stack overflows.
    no_len_checks: bool,
    // Frame pointers are useful for debugging, but they can be disabled to slightly improve performance.
    frame_pointers: bool,
    // Useful for debugging, but it can be disabled for moderate performance improvement.
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

pub fn compile(label: &str, bytecode: &Bytecode, opt: Option<CompilerOptions>) -> Result<()> {
    revmc_build::emit();

    let opt = opt.unwrap_or_default();
    let context = revmc::llvm::inkwell::context::Context::create();
    let target = revmc::Target::new(&opt.target, opt.target_cpu, opt.target_features);
    let backend = EvmLlvmBackend::new_for_target(&context, true, opt.opt_level, &target)?;
    // let backend = EvmLlvmBackend::new(&context, true, opt.opt_level)?; - alternative
    let mut compiler = EvmCompiler::new(backend);

    compiler.set_dump_to(opt.out_dir.clone());
    compiler.gas_metering(!opt.no_gas);
    unsafe { compiler.stack_bound_checks(!opt.no_len_checks) };
    compiler.frame_pointers(opt.frame_pointers);
    compiler.debug_assertions(opt.debug_assertions);

    compiler.set_module_name(label);


    compiler.inspect_stack_length(true);
    // if !stack_input.is_empty() {
    //     compiler.inspect_stack_length(true);
    // }
    let bytecode = bytecode.bytes_slice();
    compiler.translate(Some(&label), bytecode, opt.spec_id)?;
    
    let out_dir = 
        if let Some(out_dir) = opt.out_dir {
            out_dir
        } else {
            let dir = std::env::current_dir()
                .expect("Failed to get current directory")
                .join(label);
            std::fs::create_dir_all(&dir)?;
            dir
        };

    // Compile.
    let obj = out_dir.join(label).with_extension("o");
    compiler.write_object_to_file(&obj)?;

    if !obj.exists() {
        return Err(eyre::eyre!("Failed to compile object file"));
    }

    // Link.
    if !opt.no_link {
        let so = out_dir.join("a.so");
        let linker = revmc::Linker::new();
        linker.link(&so, [obj.to_str().unwrap()])?;
        ensure!(so.exists(), "Failed to link object file");
        eprintln!("Linked shared object file to {}", so.display());
    }

    // Alternative used in example
    // cc::Build::new()
    //     .object(&obj)
    //     .static_flag(true)
    //     .host("x86_64-unknown-linux-gnu")
    //     .target(&opt.target)
    //     .out_dir(&out_dir)
    //     .compile(&label);

    Ok(())

}

// todo: could this going out of scope be the issue?
pub fn load(label: &str) -> Result<revmc::EvmCompilerFn> {
    let path = std::env::current_dir()?.join(label).join("a.so");
    let lib = unsafe { libloading::Library::new(path) }?;
    let f: libloading::Symbol<'_, revmc::EvmCompilerFn> =
        unsafe { lib.get(label.as_bytes())? };
    Ok(*f)
}