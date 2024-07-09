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
    #[arg(required = false, help = "TxHash of the transaction to run/bench.")]
    pub tx_hash: Option<String>, 
    #[arg(required = false, help = "BlockNumber of the block to run/bench.")]
    pub block_num: Option<String>,
    #[arg(required = true, help = "aot_compiled or native")]
    pub run_type: String,
}

#[derive(Args, Debug)]
pub struct BenchArgs {
    #[arg(required = false, help = "TxHash of the transaction to run/bench.")]
    pub tx_hash: Option<String>, 
    #[arg(required = false, help = "BlockNumber of the block to run/bench.")]
    pub block_num: Option<String>,
}
