use libloading::Library;
use revm::primitives::B256;
use revmc::EvmCompilerFn;

use eyre::{OptionExt, Result};
use std::{path::PathBuf, str::FromStr};
use tracing::debug;

pub struct EvmCompilerFnLoader<'a> {
    dir_path: &'a PathBuf,
}

impl<'a> EvmCompilerFnLoader<'a> {
    pub fn new(dir_path: &'a PathBuf) -> Self {
        Self { dir_path }
    }

    pub fn load(&self, bytecode_hash: &B256) -> Result<(EvmCompilerFn, Library)> {
        let name = bytecode_hash.to_string();
        let path = self.dir_path.join(&name).join("a.so");
        let fnc = Self::load_from_path(&name, path)?;
        Ok(fnc)
    }

    pub fn load_selected(
        &self,
        bytecode_hashes: Vec<B256>,
    ) -> Vec<(B256, (EvmCompilerFn, Library))> {
        debug!(
            "Loading AOT compilations from dir {}: {bytecode_hashes:?}",
            self.dir_path.display()
        );
        bytecode_hashes
            .into_iter()
            .filter_map(|hash| match self.load(&hash) {
                Ok(fnc) => Some((hash, fnc)),
                Err(e) => {
                    tracing::error!("Failed to load AOT compilation for {hash}: {e}");
                    None
                }
            })
            .collect::<Vec<_>>()
    }

    pub fn load_all(&self) -> Result<Vec<(B256, (EvmCompilerFn, Library))>> {
        debug!(
            "Loading all AOT compilations from dir {}",
            self.dir_path.display()
        );
        let mut hash_fn_pairs = vec![];
        for entry in std::fs::read_dir(self.dir_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_str().ok_or_eyre("Invalid directory name")?;
            let hash = B256::from_str(name)?;
            let path = entry.path().join("a.so");

            match Self::load_from_path(name, path) {
                Ok(fnc) => hash_fn_pairs.push((hash, fnc)),
                Err(e) => {
                    tracing::error!("Failed to load AOT compilation for {name}: {e}");
                }
            }
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
