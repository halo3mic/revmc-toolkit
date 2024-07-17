mod build_utils;
mod evm_utils;
mod sim_utils;

pub mod build {
    pub use super::build_utils::*;
}
pub mod evm {
    pub use super::evm_utils::*;
}
pub mod sim {
    pub use super::sim_utils::*;
}

const DEFAULT_BUILD_CONFIG: &str = "revmc.build.config.json";
pub fn default_build_config_path() -> eyre::Result<std::path::PathBuf> {
    Ok(std::env::current_dir()?.join(DEFAULT_BUILD_CONFIG))
}
