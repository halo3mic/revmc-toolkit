use revm::primitives::B256;
use revmc::EvmCompilerFn;
use libloading::Library;


use std::{str::FromStr, path::PathBuf};
use eyre::{OptionExt, Result};
use tracing::debug;


pub struct EvmCompilerFnLoader<'a> {
    dir_path: &'a str
}

impl<'a> EvmCompilerFnLoader<'a> {
    pub fn new(dir_path: &'a str) -> Self {
        Self { dir_path }
    }

    pub fn load(&self, bytecode_hash: &B256) -> Result<(EvmCompilerFn, Library)> {
        let name = bytecode_hash.to_string();
        let path = PathBuf::from_str(self.dir_path)?.join(&name).join("a.so");
        let fnc = Self::load_from_path(&name, path)?;
        Ok(fnc)
    }

    pub fn load_selected(&self, bytecode_hashes: Vec<B256>) -> Result<Vec<(B256, (EvmCompilerFn, Library))>> {
        debug!("Loading AOT compilations from dir {}: {bytecode_hashes:?}", self.dir_path);
        bytecode_hashes.into_iter().map(|hash| {
            let fnc = self.load(&hash)?;
            Ok((hash, fnc))
        }).collect::<Result<Vec<_>>>()
    }

    pub fn load_all(&self) -> Result<Vec<(B256, (EvmCompilerFn, Library))>> {
        debug!("Loading all AOT compilations from dir {}", self.dir_path);
        let mut hash_fn_pairs = vec![];
        for entry in std::fs::read_dir(&self.dir_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or_eyre("Invalid directory name")?;
            let hash = B256::from_str(name)?;
            let path = entry.path().join("a.so");
            let fnc = Self::load_from_path(name, path)?;
            hash_fn_pairs.push((hash, fnc));
        }
        Ok(hash_fn_pairs)
    }

    fn load_from_path(name: &str, path: PathBuf) -> Result<(EvmCompilerFn, Library)> {
        debug!("Loading fn {name} from path {}", path.display());
        let lib = unsafe { Library::new(path) }?;
        let f: libloading::Symbol<'_, EvmCompilerFn> = unsafe { lib.get(name.as_bytes())? };
        Ok((*f, lib))
    }
}
