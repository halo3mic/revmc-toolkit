use std::path::PathBuf;


const DEFAULT_DATA_DIR: &str = ".data";

pub fn default_dir() -> PathBuf {
    std::env::current_dir()
        .expect("Failed to get current directory")
        .join(DEFAULT_DATA_DIR)
}

pub fn make_dir(dir_path: &PathBuf) -> eyre::Result<()> {
    if !dir_path.exists() {
        std::fs::create_dir_all(&dir_path)?;
    }
    Ok(())
}

pub fn bytecode_hash_str(bytecode: &[u8]) -> String {
    revm::primitives::keccak256(bytecode).to_string()
}

#[derive(serde::Deserialize, Debug, Clone)]
pub enum OptimizationLevelDeseralizable {
    None,
    Less,
    Default,
    Aggressive,
}

impl Into<revmc::OptimizationLevel> for OptimizationLevelDeseralizable {
    fn into(self) -> revmc::OptimizationLevel {
        match self {
            OptimizationLevelDeseralizable::None => revmc::OptimizationLevel::None,
            OptimizationLevelDeseralizable::Less => revmc::OptimizationLevel::Less,
            OptimizationLevelDeseralizable::Default => revmc::OptimizationLevel::Default,
            OptimizationLevelDeseralizable::Aggressive => revmc::OptimizationLevel::Aggressive,
        }
    }
}
