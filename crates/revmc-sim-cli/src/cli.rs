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
    Build(BuildArgs),
    Run(RunArgs),
    Bench(BenchArgs)
}

#[derive(Args, Debug)]
pub struct BuildArgs {
    // todo
}

#[derive(Args, Debug)]
pub struct RunArgs {
    #[arg(short, long, help = "TxHash of the transaction to run/bench.")]
    pub tx_hash: Option<String>, 
    #[arg(short, long, help = "BlockNumber of the block to run/bench.")]
    pub block_num: Option<String>,
    #[arg(short, long, help = "aot_compiled or native")]
    pub run_type: String,
}

#[derive(Args, Debug)]
pub struct BenchArgs {
    #[arg(short, long, help = "TxHash of the transaction to run/bench.")]
    pub tx_hash: Option<String>, 
    #[arg(short, long, help = "BlockNumber of the block to run/bench.")]
    pub block_num: Option<String>,
}
