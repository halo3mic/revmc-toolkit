use clap::{Parser, Subcommand, Args};


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
    // Run(RunArgsCli), // todo
    #[command(subcommand)]
    Bench(BenchType),
}

#[derive(Subcommand)]
pub enum BenchType {
    Tx { tx_hash: String },
    Block(BenchBlockArgsCli),
    Call,
    BlockRange(BlockRangeArgsCli),
}

#[derive(Args, Debug)]
pub struct BuildArgsCli {
    // todo
}

#[derive(Args, Debug)]
pub struct BenchBlockArgsCli {
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
    #[arg(short, long, help = "Number of samples taken from the range. If ommited the whole range is compared.")]
    pub sample_size: Option<u32>,
    #[arg(short, long, help = "Path to dir where measurements will be stored.")]
    pub out_dir: Option<String>,
    #[arg(long, help = "Warmup time [ms].")]
    pub warmup_ms: Option<u32>,
    #[arg(long, help = "Measurment time [ms].")]
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