mod build_utils;
pub mod bench;
pub mod sim;

pub mod build {
    pub use super::build_utils::*;
}


const DEFAULT_BUILD_CONFIG: &str = "revmc.build.config.json";
pub fn default_build_config_path() -> eyre::Result<std::path::PathBuf> {
    Ok(std::env::current_dir()?.join(DEFAULT_BUILD_CONFIG))
}

pub fn make_dir(dir_path: &std::path::PathBuf) -> eyre::Result<()> {
    if !dir_path.exists() {
        std::fs::create_dir_all(&dir_path)?;
    }
    Ok(())
}

pub fn epoch_now() -> eyre::Result<u64> {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    Ok(epoch)
}