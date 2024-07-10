use revm::primitives::B256;
use revmc::EvmCompilerFn;
use libloading::Library;

use std::{str::FromStr, path::PathBuf};
use eyre::{OptionExt, Result};


pub struct EvmCompilerFnLoader {
    dir_path: String
}

impl EvmCompilerFnLoader {
    pub fn new(dir_path: String) -> Self {
        Self { dir_path }
    }

    pub fn load(&self, bytecode_hash: &B256) -> Result<(EvmCompilerFn, Library)> {
        let name = bytecode_hash.to_string();
        let path = PathBuf::from_str(&name)?.join("a.so");
        let fnc = Self::load_from_path(&name, path)?;
        Ok(fnc)
    }

    pub fn load_selected(&self, bytecode_hashes: Vec<B256>) -> Result<Vec<(B256, (EvmCompilerFn, Library))>> {
        bytecode_hashes.into_iter().map(|hash| {
            let fnc = self.load(&hash)?;
            Ok((hash, fnc))
        }).collect::<Result<Vec<_>>>()
    }

    pub fn load_all(&self) -> Result<Vec<(B256, (EvmCompilerFn, Library))>> {
        let mut hash_fn_pairs = vec![];
        println!("Loading from path: {:?}", self.dir_path);
        for entry in std::fs::read_dir(&self.dir_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                return Err(eyre::eyre!("Found non-directory entry at {:?}", entry.path()));
            }
            println!("Loading at path: {:?}", entry.path().display());
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
        let lib = unsafe { Library::new(path) }?;
        let f: libloading::Symbol<'_, EvmCompilerFn> = unsafe { lib.get(name.as_bytes())? };
        Ok((*f, lib))
    }
}
