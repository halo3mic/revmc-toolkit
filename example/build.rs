use revmc::{
    primitives::{hex, SpecId},
    EvmCompiler, EvmLlvmBackend, OptimizationLevel, Result,
};
use std::path::PathBuf;

#[allow(dead_code)]
const FIBONACCI_CODE: &[u8] =
    &hex!("5f355f60015b8215601a578181019150909160019003916005565b9150505f5260205ff3");
#[allow(dead_code)]
const FIBONACCI_HASH: [u8; 32] =
    hex!("ab1ad1211002e1ddb8d9a4ef58a902224851f6a0273ee3e87276a8d21e649ce8");

fn main() -> Result<()> {
    println!("Building 👷‍♂️");
    // Emit the configuration to run compiled bytecodes.
    // This not used if we are only using statically linked bytecodes.
    revmc_build::emit();

    // Compile and statically link a bytecode.
    let name = "fib";
    let bytecode = FIBONACCI_CODE;

    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let context = revmc::llvm::inkwell::context::Context::create();
    let backend = EvmLlvmBackend::new(&context, true, OptimizationLevel::Aggressive)?;
    let mut compiler = EvmCompiler::new(backend);
    compiler.translate(Some(name), bytecode, SpecId::CANCUN)?;
    let object = out_dir.join(name).with_extension("o");
    compiler.write_object_to_file(&object)?;

    cc::Build::new().object(&object).static_flag(true).compile(name);

    Ok(())
}