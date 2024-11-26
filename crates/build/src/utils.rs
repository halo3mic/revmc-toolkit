use std::path::PathBuf;

const DEFAULT_DATA_DIR: &str = ".data/aot_compile";

pub fn default_dir() -> PathBuf {
    std::env::current_dir()
        .expect("Failed to get current directory")
        .join(DEFAULT_DATA_DIR)
}

pub fn bytecode_hash_str(bytecode: &[u8]) -> String {
    revm::primitives::keccak256(bytecode).to_string()
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, Default)]
pub enum OptimizationLevelDeseralizable {
    None,
    Less,
    #[default]
    Default,
    Aggressive,
}

impl From<OptimizationLevelDeseralizable> for revmc::OptimizationLevel {
    fn from(value: OptimizationLevelDeseralizable) -> Self {
        match value {
            OptimizationLevelDeseralizable::None => revmc::OptimizationLevel::None,
            OptimizationLevelDeseralizable::Less => revmc::OptimizationLevel::Less,
            OptimizationLevelDeseralizable::Default => revmc::OptimizationLevel::Default,
            OptimizationLevelDeseralizable::Aggressive => revmc::OptimizationLevel::Aggressive,
        }
    }
}

impl TryFrom<u8> for OptimizationLevelDeseralizable {
    type Error = eyre::Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(OptimizationLevelDeseralizable::None),
            1 => Ok(OptimizationLevelDeseralizable::Less),
            2 => Ok(OptimizationLevelDeseralizable::Default),
            3 => Ok(OptimizationLevelDeseralizable::Aggressive),
            _ => Err(eyre::eyre!("Invalid optimization level")),
        }
    }
}
