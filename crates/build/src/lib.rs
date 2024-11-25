mod compiler;
mod utils;

use eyre::Result;
use rayon::prelude::*;
use std::path::Path;

pub use compiler::{Compiler, CompilerOptions, JitCompileCtx, JitCompileOut, PtrWrapper};
pub use utils::{default_dir, OptimizationLevelDeseralizable};

pub fn compile_contracts_aot(
    args: &[Vec<u8>],
    fallback_opt: Option<CompilerOptions>,
) -> Result<Vec<Result<()>>> {
    // todo: try compiling multiple contracts with the same ctx and compiler
    let opt = fallback_opt.unwrap_or_default();
    let compiled_contracts = load_compiled(&opt.out_dir).unwrap_or_default();
    let compiler: Compiler = opt.into();
    Ok(args
        .par_iter()
        .filter(|arg| !compiled_contracts.contains(&utils::bytecode_hash_str(arg)))
        .map(|arg| compiler.compile_aot(arg))
        .collect())
}

pub fn compile_contracts_jit(
    args: &[Vec<u8>],
    fallback_opt: Option<CompilerOptions>,
) -> Result<JitCompileOut> {
    let compiler: Compiler = fallback_opt.unwrap_or_default().into();
    args.par_chunks(10) // todo: make configurable
        .map(|chunk| compiler.compile_jit_many(chunk))
        .reduce_with(|acc, res| {
            let mut acc = acc?;
            acc.merge(res?);
            Ok(acc)
        })
        .unwrap_or_else(|| Ok(JitCompileOut::default()))
}

fn load_compiled(path: &Path) -> Result<Vec<String>> {
    let vec = std::fs::read_dir(path)?
        .map(|res| {
            res.map(|e| {
                let path = e.path();
                let file_name = path.file_name().unwrap().to_owned().into_string().unwrap();
                file_name
            })
        })
        .collect::<Result<Vec<_>, std::io::Error>>()?;
    Ok(vec)
}
