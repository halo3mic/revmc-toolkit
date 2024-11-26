use crate::{benches::BlockRangeArgs, utils, BlockPart};
use clap::{Args, Parser, Subcommand};
use eyre::Result;
use revm::primitives::Bytes;
use revmc_toolkit_sim::gas_guzzlers::GasGuzzlerConfig;
use revmc_toolkit_utils::rnd as rnd_utils;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(subcommand)]
    Run(RunArgsCli),
    #[command(subcommand)]
    Bench(Box<BenchType>),
}

#[derive(Subcommand)]
pub enum BenchType {
    Tx {
        tx_hash: String,
        #[arg(long)]
        comp_opt_level: Option<u8>,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Block {
        #[arg(long)]
        comp_opt_level: Option<u8>,
        #[command(flatten)]
        block_args: BlockArgsCli,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Call {
        comp_opt_level: Option<u8>,
    },
    BlockRange {
        #[arg(long)]
        comp_opt_level: Option<u8>,
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
        comp_opt_level: Option<u8>,
        #[arg(long)]
        run_type: String,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Block {
        #[arg(long)]
        comp_opt_level: Option<u8>,
        #[command(flatten)]
        block_args: BlockArgsCli,
        #[arg(long)]
        run_type: String,
        #[command(subcommand)]
        bytecode_selection: Option<BytecodeSelectionCli>,
    },
    Call {
        #[arg(long)]
        comp_opt_level: Option<u8>,
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
    pub size_limit: usize,
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
    #[arg(
        short,
        long,
        help = "Number of samples taken from the range. If omitted the whole range is compared."
    )]
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
    #[arg(
        default_value = "false",
        long,
        help = "If present will run single random transaction per block. If block-chunk is set, it will pick the transaction from it."
    )]
    pub run_rnd_txs: bool,
    #[arg(long, help = "Comma-separated list of block numbers to blacklist.")]
    pub blacklist: Option<String>,
    #[arg(long, help = "Compiler optimization level.")]
    pub comp_opt_level: Option<u8>,
}

impl From<GasGuzzlersCli> for (GasGuzzlerConfig, usize) {
    fn from(cli: GasGuzzlersCli) -> (GasGuzzlerConfig, usize) {
        (
            GasGuzzlerConfig {
                start_block: cli.start_block,
                end_block: cli.end_block,
                sample_size: cli.sample_size,
                seed: cli.seed.map(hashed),
            },
            cli.size_limit,
        )
    }
}

fn hashed<T: AsRef<str>>(seed_str: T) -> [u8; 32] {
    revm::primitives::keccak256(seed_str.as_ref().as_bytes()).0
}

impl BlockRangeArgsCli {
    fn block_iter(&self) -> Result<Vec<u64>> {
        let (start, end, range_size) = self.start_end_range()?;
        let blacklist = self.parse_blacklist()?;
        let block_iter = if let Some(sample_size) = self.sample_size {
            if sample_size > range_size {
                return Err(eyre::eyre!("Invalid sample size"));
            }
            let seed = self.hashed_seed();
            rnd_utils::random_sequence_with_blacklist(
                start,
                end,
                sample_size as usize,
                seed,
                blacklist,
            )?
        } else {
            (start..end).collect()
        };
        Ok(block_iter)
    }

    fn out_dir_path(&self) -> Result<PathBuf> {
        let default_out_dir = std::env::current_dir()?.join(".data/measurements");
        utils::make_dir(&default_out_dir)?;

        let label = match self.label {
            Some(ref label) => label.clone(),
            None => {
                let (start, end, range_size) = self.start_end_range()?;
                let range_size = self.sample_size.unwrap_or(range_size);
                let epoch_now = utils::epoch_now()?;
                format!("f{start}t{end}s{range_size}e{epoch_now}")
            }
        };
        let out_dir_path = self
            .out_dir
            .clone()
            .map(PathBuf::from)
            .unwrap_or(default_out_dir)
            .join(label);

        Ok(out_dir_path)
    }

    fn block_chunk(&self) -> Option<BlockPart> {
        if let Some(tob) = self.tob_block_chunk {
            Some(BlockPart::TOB(tob))
        } else {
            self.bob_block_chunk.map(BlockPart::BOB)
        }
    }

    fn start_end_range(&self) -> Result<(u64, u64, u32)> {
        let [start, end, ..] = self.block_range.split_terminator("..").collect::<Vec<_>>()[..]
        else {
            return Err(eyre::eyre!("Invalid block range format"));
        };
        let start = start.parse::<u64>()?;
        let end = end.parse::<u64>()?;
        if end < start {
            return Err(eyre::eyre!("End block must be greater than start block"));
        }
        let range_size = (end - start) as u32;
        Ok((start, end, range_size))
    }

    fn hashed_seed(&self) -> Option<[u8; 32]> {
        self.rnd_seed.as_ref().map(hashed)
    }

    fn parse_blacklist(&self) -> Result<Vec<u64>> {
        self.blacklist
            .as_ref()
            .map(|blacklist| {
                blacklist
                    .split(',')
                    .map(|num| num.parse::<u64>().map_err(Into::into))
                    .collect::<Result<Vec<_>>>()
            })
            .transpose()
            .map(|blacklist| blacklist.unwrap_or_default())
    }
}

impl TryInto<BlockRangeArgs> for BlockRangeArgsCli {
    type Error = eyre::Error;

    fn try_into(self) -> Result<BlockRangeArgs, Self::Error> {
        Ok(BlockRangeArgs {
            measurement_ms: self.measurement_ms.unwrap_or(5_000),
            warmup_ms: self.warmup_ms.unwrap_or(3_000),
            block_chunk: self.block_chunk(),
            block_iter: self.block_iter()?,
            out_dir_path: self.out_dir_path()?,
            run_rnd_txs: self.run_rnd_txs,
            seed: self.hashed_seed(),
            comp_opt_level: self.comp_opt_level.unwrap_or_default().try_into()?,
        })
    }
}
