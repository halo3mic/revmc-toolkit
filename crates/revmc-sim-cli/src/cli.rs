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
    Run(RunArgsCli),
    Bench(BenchArgsCli),
    BlockRange(BlockRangeArgsCli),
}

#[derive(Args, Debug)]
pub struct BuildArgsCli {
    // todo
}

#[derive(Args, Debug)]
pub struct RunArgsCli {
    #[arg(short, long, help = "TxHash of the transaction to run/bench.")]
    pub tx_hash: Option<String>, 
    #[arg(short, long, help = "BlockNumber of the block to run/bench.")]
    pub block_num: Option<String>,
    #[arg(short, long, help = "aot_compiled or native")]
    pub run_type: String,
}

#[derive(Args, Debug)]
pub struct BenchArgsCli {
    #[arg(short, long, help = "TxHash of the transaction to run/bench.")]
    pub tx_hash: Option<String>, 
    #[arg(short, long, help = "BlockNumber of the block to run/bench.")]
    pub block_num: Option<String>,
}

#[derive(Args, Debug)]
pub struct BlockRangeArgsCli {
    #[arg(help = "Block range in format start..end")]
    pub block_range: String,
    #[arg(help = "Label of run")]
    pub label: Option<String>,
    #[arg(short, long, help = "Number of samples taken from the range. If ommited the whole range is compared.")]
    pub sample_size: Option<u32>,
    #[arg(short, long, help = "Path to dir where measurments will be stored.")]
    pub out_dir: Option<String>,
    #[arg(short, long, help = "Warmup iterations.")]
    pub warmup_iter: Option<u32>,
    #[arg(short, long, help = "Bench iterations.")]
    pub bench_iter: Option<u32>,
}