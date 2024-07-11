use revmc_sim_build::{self, ConfigFile};
use eyre::Result;


const DEFAULT_CONFIG_NAME: &str = "revmc.build.config.json";

fn main() -> Result<()> {
    let config = read_config()?;
    revmc_sim_build::compile_contracts(config.contracts, config.fallback_config)?;
    Ok(())
}

fn read_config() -> Result<ConfigFile> {
    let current_dir = std::env::current_dir()?;
    let config_path = current_dir.join(DEFAULT_CONFIG_NAME);
    ConfigFile::from_path(config_path)
}