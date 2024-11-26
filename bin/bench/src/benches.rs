use criterion::Criterion;
use eyre::{OptionExt, Result};
use rayon::prelude::{IntoParallelIterator, ParallelIterator};
use revmc_toolkit_build::CompilerOptions;
use std::{path::PathBuf, time::Duration};
use tracing::{info, span, warn, Level};

use reth_db::DatabaseEnv;
use reth_provider::{BlockReader, ProviderFactory};
use revm::primitives::{Bytes, B256};

use crate::utils::{
    bench::{self as bench_utils, RunConfig},
    sim::{self as sim_utils, BytecodeSelection, SimCall, SimConfig, SimRunType},
};
use revmc_toolkit_build::OptimizationLevelDeseralizable;
use revmc_toolkit_load::{EvmCompilerFns, RevmcExtCtx};
use revmc_toolkit_sim::sim_builder::{BlockPart, Simulation, StateProviderCacheDB};
use revmc_toolkit_utils::{evm as evm_utils, rnd as rnd_utils};

// todo: sample_size and measurement_time as args
// todo: add jit optionally

impl RunConfig<PathBuf, BytecodeSelection> {
    pub fn new(
        aot_dir_path: PathBuf,
        reth_db_path: PathBuf,
        compile_selection: BytecodeSelection,
    ) -> Self {
        Self {
            aot_dir_path,
            reth_db_path,
            compile_selection,
            comp_opt_level: Default::default(),
        }
    }

    pub fn bench_tx(&self, tx_hash: B256) -> Result<()> {
        let span = span!(Level::INFO, "bench_tx");
        let _guard = span.enter();
        info!("TxHash: {:?}", tx_hash);

        self.bench_variant(
            |_provider_factory: &ProviderFactory<DatabaseEnv>| Ok(vec![tx_hash]),
            |sim_config: SimConfig<ProviderFactory<DatabaseEnv>>| sim_config.make_tx_sim(tx_hash),
        )
    }

    pub fn bench_block(&self, block_num: u64, block_chunk: Option<BlockPart>) -> Result<()> {
        let span = span!(Level::INFO, "bench_block");
        let _guard = span.enter();
        info!("Block: {:?}", block_num);

        self.bench_variant(
            |provider_factory: &ProviderFactory<DatabaseEnv>| {
                txs_for_block(provider_factory, block_num)
            },
            |sim_config: SimConfig<ProviderFactory<DatabaseEnv>>| {
                sim_config.make_block_sim(block_num, block_chunk)
            },
        )
    }

    pub fn bench_block_range(&self, args: BlockRangeArgs) -> Result<()> {
        let span = span!(Level::INFO, "bench_block_range");
        let _guard = span.enter();
        let provider_factory = evm_utils::make_provider_factory(&self.reth_db_path)?;

        BlockRangeRunner::new(
            args,
            provider_factory,
            self.aot_dir_path.clone(),
            &self.compile_selection,
        )?
        .run()
    }

    pub fn bench_variant<FTx, FSim>(&self, build_txs_fn: FTx, build_sim_fn: FSim) -> Result<()>
    where
        FSim: Fn(
            SimConfig<ProviderFactory<DatabaseEnv>>,
        ) -> Result<Simulation<RevmcExtCtx, StateProviderCacheDB>>,
        FTx: Fn(&ProviderFactory<DatabaseEnv>) -> Result<Vec<B256>>,
    {
        let provider_factory = evm_utils::make_provider_factory(&self.reth_db_path)?;
        let txs = build_txs_fn(&provider_factory)?;
        let bytecodes = self
            .compile_selection
            .bytecodes(provider_factory.clone(), Some(txs.clone().into()))?;

        let mut criterion = Criterion::default()
            .sample_size(100)
            .measurement_time(Duration::from_secs(5));
        for (symbol, run_type) in [
            ("native", SimRunType::Native),
            // ("jit", SimRunType::JITCompiled),
            ("aot", SimRunType::AOTCompiled),
        ] {
            info!("Running {}", symbol.to_uppercase());

            let ext_ctx = sim_utils::make_ext_ctx(&run_type, &bytecodes, Some(self.compile_opt()))?
                .with_touch_tracking();
            let sim_config = SimConfig::new(provider_factory.clone(), ext_ctx);
            let mut sim = build_sim_fn(sim_config)?;

            bench_utils::check_tx_sim_validity(
                &provider_factory,
                &mut sim,
                txs.clone(),
                matches!(run_type, SimRunType::Native),
            )?;

            criterion.bench_function(&format!("sim_{symbol}"), |b| b.iter(|| sim.run()));
        }
        Ok(())
    }
}

impl<T, U> RunConfig<T, U> {
    pub fn bench_call(&self, call: SimCall, call_input: Bytes) -> Result<()> {
        let span = span!(Level::INFO, "bench_call");
        let _guard = span.enter();
        info!("Call: {:?}", call);
        let mut criterion = Criterion::default()
            .sample_size(100)
            .measurement_time(Duration::from_secs(5));
        let mut group = criterion.benchmark_group("call_benchmarks");

        for (symbol, run_type) in [
            ("jit", SimRunType::JITCompiled),
            ("aot", SimRunType::AOTCompiled),
            ("native", SimRunType::Native),
        ] {
            info!("Running {}", symbol.to_uppercase());

            let bytecode: Vec<_> = call.bytecode().original_bytes().into();
            let ext_ctx =
                sim_utils::make_ext_ctx(&run_type, &[bytecode], Some(self.compile_opt()))?;
            let mut sim = SimConfig::from(ext_ctx).make_call_sim(call, call_input.clone())?;

            group.bench_function(format!("sim_call_{symbol}"), |b| b.iter(|| sim.run()));
        }
        group.finish();
        Ok(())
    }
}

impl<T> RunConfig<T, BytecodeSelection> {
    pub fn set_bytecode_selection_opt<S: Into<BytecodeSelection>>(&mut self, selection: Option<S>) {
        if let Some(selection) = selection {
            self.set_bytecode_selection(selection.into());
        }
    }

    pub fn set_bytecode_selection(&mut self, selection: BytecodeSelection) {
        self.compile_selection = selection;
    }

    pub fn set_compile_opt_level(&mut self, level: Option<u8>) -> Result<()> {
        if let Some(level) = level {
            self.comp_opt_level = level.try_into()?;
        }
        Ok(())
    }
}

fn txs_for_block(
    provider_factory: &ProviderFactory<DatabaseEnv>,
    block_num: u64,
) -> Result<Vec<B256>> {
    let block = provider_factory
        .block(block_num.into())?
        .ok_or_eyre("Block not found")?;
    Ok(block.body.iter().map(|tx| tx.hash).collect())
}

#[derive(Debug, serde::Serialize)]
enum MeasureId {
    Block(u64),
    Tx(B256),
}

#[derive(Debug, serde::Serialize)]
struct MeasureRecord {
    id: MeasureId,
    run_type: String,
    exe_time: f64,
    err: Option<String>,
}

#[derive(serde::Serialize)]
pub struct BlockRangeArgs {
    #[serde(skip)]
    pub block_iter: Vec<u64>,
    #[serde(skip)]
    pub out_dir_path: PathBuf,
    pub warmup_ms: u32,
    pub measurement_ms: u32,
    pub block_chunk: Option<BlockPart>,
    pub run_rnd_txs: bool,
    pub seed: Option<[u8; 32]>,
    pub comp_opt_level: OptimizationLevelDeseralizable,
}

use csv::{Writer, WriterBuilder};
use std::fs::{File, OpenOptions};
use std::sync::Mutex;

struct BlockRangeRunner {
    args: BlockRangeArgs,
    provider_factory: ProviderFactory<DatabaseEnv>,
    aot_dir_path: PathBuf,
    writer: Mutex<csv::Writer<std::fs::File>>,
    bytecodes: Vec<Vec<u8>>,
}

impl BlockRangeRunner {
    fn new(
        args: BlockRangeArgs,
        provider_factory: ProviderFactory<DatabaseEnv>,
        aot_dir_path: PathBuf,
        bytecode_selection: &BytecodeSelection,
    ) -> Result<Self> {
        let writer = Mutex::new(Self::create_csv_writer(&args, bytecode_selection)?);
        let bytecodes = bytecode_selection
            .bytecodes(provider_factory.clone(), Some((&args.block_iter).into()))?;
        Ok(Self {
            args,
            provider_factory,
            aot_dir_path,
            writer,
            bytecodes,
        })
    }

    // we make this step parallel so that in case of a failure we get as much block matches accross different exe types as possible
    // jit is too slow to be included here
    fn run(&mut self) -> Result<()> {
        [
            ("aot", SimRunType::AOTCompiled),
            ("native", SimRunType::Native),
        ]
        .into_par_iter()
        .map(|(symbol, run_type)| self.process_blocks(symbol, &run_type))
        .collect::<Result<Vec<_>>>()?;

        info!("Finished comparing block range ✨");
        info!(
            "The records are written to {}",
            self.args.out_dir_path.display()
        );
        Ok(())
    }

    fn process_blocks(&self, symbol: &str, run_type: &SimRunType) -> Result<()> {
        let compiled_fns = self.compiled_fns_for_run_type(run_type)?;
        self.args
            .block_iter
            .clone()
            .into_par_iter()
            .map(|block_num| {
                self.process_single_block(block_num, symbol, run_type, compiled_fns.clone())
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(())
    }

    fn process_single_block(
        &self,
        block_num: u64,
        symbol: &str,
        run_type: &SimRunType,
        compiled_fns_cache: EvmCompilerFns,
    ) -> Result<()> {
        info!("Running {} for block {block_num}", symbol.to_uppercase());

        let sim_opt = self.create_sim_for_block(block_num, compiled_fns_cache)?;
        if let Some((mut sim, m_id)) = sim_opt {
            let check_res = bench_utils::check_tx_sim_validity(
                &self.provider_factory,
                &mut sim,
                txs_for_block(&self.provider_factory, block_num)?,
                matches!(run_type, SimRunType::Native),
            );
            if let Err(e) = &check_res {
                warn!("Check failed for block {block_num} with: {e}");
            }
            let exe_time = bench_utils::measure_execution_time(
                || sim.run(),
                self.args.warmup_ms,
                self.args.measurement_ms,
            );
            self.write_measurement(MeasureRecord {
                run_type: symbol.to_string(),
                id: m_id,
                exe_time,
                err: check_res.err().map(|e| e.to_string()),
            })?;
        }
        Ok(())
    }

    fn create_sim_for_block(
        &self,
        block_num: u64,
        compiled_fns: EvmCompilerFns,
    ) -> Result<Option<(Simulation<RevmcExtCtx, StateProviderCacheDB>, MeasureId)>> {
        let sim_config = SimConfig::new(
            self.provider_factory.clone(),
            RevmcExtCtx::from(compiled_fns).with_touch_tracking(),
        );
        if self.args.run_rnd_txs {
            self.create_rnd_tx_sim(sim_config, block_num)
        } else {
            let sim = sim_config.make_block_sim(block_num, self.args.block_chunk)?;
            let m_id = MeasureId::Block(block_num);
            Ok(Some((sim, m_id)))
        }
    }

    fn create_rnd_tx_sim(
        &self,
        sim_config: SimConfig<ProviderFactory<DatabaseEnv>>,
        block_num: u64,
    ) -> Result<Option<(Simulation<RevmcExtCtx, StateProviderCacheDB>, MeasureId)>> {
        let block = self
            .provider_factory
            .block(block_num.into())?
            .ok_or_eyre("Block not found")?;
        if block.body.is_empty() {
            warn!("Found empty block {}, skipping", block_num);
            return Ok(None);
        }
        let txs_len = self
            .args
            .block_chunk
            .map(|chunk| match chunk {
                BlockPart::TOB(c) => (block.body.len() as f32 * c) as usize,
                BlockPart::BOB(c) => (block.body.len() as f32 * (1. - c)) as usize,
            })
            .unwrap_or(block.body.len());
        let tx_index = rnd_utils::random_sequence(0, txs_len, 1, self.args.seed)?[0];
        let tx_hash = block.body[tx_index].hash;
        info!("Running random tx: {tx_hash:?}");

        Ok(Some((
            sim_config.make_tx_sim(tx_hash)?,
            MeasureId::Tx(tx_hash),
        )))
    }

    fn compiled_fns_for_run_type(&self, run_type: &SimRunType) -> Result<EvmCompilerFns> {
        if matches!(run_type, SimRunType::Native) {
            Ok(EvmCompilerFns::default())
        } else {
            info!(
                "Aquiring {} compiled fns for {run_type:?}",
                self.bytecodes.len()
            );
            sim_utils::make_compiled_fns(run_type, &self.bytecodes, Some(self.compile_opt()))
        }
    }

    // fn bytecodes_for_range(
    //     provider_factory: ProviderFactory<DatabaseEnv>,
    //     bytecode_selection: &BytecodeSelection,
    //     block_iter: &[u64],
    // ) -> Result<Vec<Vec<u8>>> {
    //     Ok(
    //         if let BytecodeSelection::GasGuzzlers {
    //             config: gconfig,
    //             size_limit,
    //         } = bytecode_selection
    //         {
    //             gconfig
    //                 .find_gas_guzzlers(provider_factory)?
    //                 .into_top_guzzlers(Some(*size_limit))
    //         } else {
    //             bytecode_touches::find_touched_bytecode_blocks(provider_factory, block_iter)?
    //                 .into_iter()
    //                 .collect::<Vec<_>>()
    //         },
    //     )
    // }

    fn write_measurement(&self, record: MeasureRecord) -> Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.serialize(record)?;
        writer.flush()?;
        Ok(())
    }

    fn create_csv_writer(
        args: &BlockRangeArgs,
        bytecode_selection: &BytecodeSelection,
    ) -> Result<Writer<File>> {
        revmc_toolkit_utils::misc::make_dir(&args.out_dir_path)?;
        let config_path = args.out_dir_path.join("config.json");
        serde_json::to_writer_pretty(
            std::fs::File::create(&config_path)?,
            &serde_json::json!({
                "args": args,
                "bytecode_selection": bytecode_selection,
            }),
        )?;
        let data_path = args.out_dir_path.join("data.csv");
        let file_exists = data_path.exists();
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(data_path)?;
        let writer = WriterBuilder::new()
            .has_headers(!file_exists)
            .from_writer(file);
        Ok(writer)
    }

    fn compile_opt(&self) -> CompilerOptions {
        CompilerOptions::default()
            .with_out_dir(self.aot_dir_path.clone())
            .with_opt_lvl(self.args.comp_opt_level.clone())
    }
}
