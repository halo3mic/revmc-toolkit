#[derive(clap::Parser)]
pub struct Cli {
    #[arg(short, long)]
    pub start_block: u64,
    #[arg(short, long)]
    pub end_block: u64,
    #[arg(short, long)]
    pub sample_size: u64,
    #[arg(short, long)]
    pub take: Option<usize>,
    #[arg(
        short,
        long,
        help = "Only elements up to this value are returned. It represents cumulative proportion of gas used."
    )]
    pub gas_limit: Option<f64>,
    #[arg(
        short,
        long,
        help = "If true instead of bytecode hash of bytecode will be returned"
    )]
    pub hashed: bool,
}
