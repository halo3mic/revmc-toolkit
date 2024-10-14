use clap::{Parser, Subcommand, Args};
use revm::primitives::Bytes;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Build(BuildArgsCli),
    #[command(subcommand)]
    Run(RunArgsCli),
    #[command(subcommand)]
    Bench(BenchType),
}

#[derive(Subcommand)]
pub enum BenchType {
    Tx {
        tx_hash: String,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Block {
        #[command(flatten)]
        block_args: BlockArgsCli,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Call,
    BlockRange {
        #[command(flatten)]
        block_range_args: BlockRangeArgsCli,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
}

#[derive(Subcommand)]
pub enum RunArgsCli {
    Tx { 
        tx_hash: String,
        #[arg(long)]
        run_type: String,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Block {
        #[command(flatten)]
        block_args: BlockArgsCli,
        #[arg(long)]
        run_type: String,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Call {
        #[arg(long)]
        run_type: String,
        #[arg(long)]
        input: Option<Bytes>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BytecodeSelectionCli {
    Selected,
    GasGuzzlers(GasGuzzlersCli),
}

#[derive(Args, Debug)]
pub struct GasGuzzlersCli { 
    #[arg(short, long, help = "Start block for gas guzzlers selection.")]
    pub start_block: Option<u64>,
    #[arg(short, long, help = "End block for gas guzzlers selection.")]
    pub end_block: Option<u64>,
    #[arg(long, help = "Sample size for gas guzzlers selection.")]
    pub sample_size: Option<u64>,
    #[arg(long, help = "Seed for random number generator.")]
    pub seed: Option<String>,
    #[arg(long, help = "Size limit for gas guzzlers selection.")]
    pub size_limit: usize
}

#[derive(Args, Debug)]
pub struct BuildArgsCli {
    // todo
}

#[derive(Args, Debug)]
pub struct BlockArgsCli {
    pub block_num: u64,
    #[arg(long, help = "Proportion of the block to use - top of the block.")]
    pub tob_block_chunk: Option<f32>,
    #[arg(long, help = "Proportion of the block to use - bottom of the block.")]
    pub bob_block_chunk: Option<f32>,
}

#[derive(Args, Debug)]
pub struct BenchArgsCli {
    #[arg(short, long, help = "TxHash of the transaction to run/bench.")]
    pub tx_hash: Option<String>, 
    #[arg(short, long, help = "BlockNumber of the block to run/bench.")]
    pub block_num: Option<String>,
    #[arg(long, help = "Proportion of the block to use - top of the block.")]
    pub tob_block_chunk: Option<f32>,
    #[arg(long, help = "Proportion of the block to use - bottom of the block.")]
    pub bob_block_chunk: Option<f32>,
}

#[derive(Args, Debug)]
pub struct BlockRangeArgsCli {
    #[arg(help = "Block range in format start..end")]
    pub block_range: String,
    #[arg(help = "Label of run")]
    pub label: Option<String>,
    #[arg(short, long, help = "Number of samples taken from the range. If omitted the whole range is compared.")]
    pub sample_size: Option<u32>,
    #[arg(short, long, help = "Path to dir where measurements will be stored.")]
    pub out_dir: Option<String>,
    #[arg(long, help = "Warmup time [ms].")]
    pub warmup_ms: Option<u32>,
    #[arg(long, help = "Measurement time [ms].")]
    pub measurement_ms: Option<u32>,
    #[arg(long, help = "Proportion of the block to use - top of the block.")]
    pub tob_block_chunk: Option<f32>,
    #[arg(long, help = "Proportion of the block to use - bottom of the block.")]
    pub bob_block_chunk: Option<f32>,
    #[arg(long, help = "Seed for random number generator.")]
    pub rnd_seed: Option<String>,
    #[arg(default_value = "false", long, help = "If present will run single random transaction per block. If block-chunk is set, it will pick the transaction from it.")]
    pub run_rnd_txs: bool,
}

use revmc_toolkit_sim::gas_guzzlers::GasGuzzlerConfig;

impl Into<(GasGuzzlerConfig, usize)> for GasGuzzlersCli {
    fn into(self) -> (GasGuzzlerConfig, usize) {
        (GasGuzzlerConfig {
            start_block: self.start_block,
            end_block: self.end_block,
            sample_size: self.sample_size,
            seed: self.seed.map(hashed),
        }, self.size_limit)
    }
}

fn hashed(seed_str: String) -> [u8; 32] {
    revm::primitives::keccak256(seed_str.as_bytes()).0
}

// fn seed_str_to_bytes(seed_str: String) -> [u8; 32] {
//     let mut seed = [0u8; 32];
//     let bytes = hex::decode(seed_str).expect("Invalid seed");
//     seed.copy_from_slice(&bytes);
//     seed
// }

