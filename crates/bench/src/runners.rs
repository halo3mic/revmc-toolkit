
use std::path::PathBuf;
use eyre::{OptionExt, Result};
use revm::primitives::{B256, Bytes};
use reth_provider::BlockReader;

use revmc_toolkit_utils::evm::make_provider_factory;
use revmc_toolkit_sim::sim_builder::BlockPart;
use revmc_toolkit_load::RevmcExtCtx;
use crate::utils::{
    sim::{self as sim_utils, BytecodeSelection, SimCall, SimConfig, SimRunType},
    bench::{RunConfig, self as bench_utils},
};


impl RunConfig<PathBuf, BytecodeSelection> {

    pub fn run_tx(&self, tx_hash: B256, run_type: SimRunType) -> Result<()> {
        println!("TxHash: {tx_hash:?}");

        let provider_factory = make_provider_factory(&self.reth_db_path)?;
        let (ext_ctx, is_native_exe) = 
            match &run_type {
                SimRunType::AOTCompiled | SimRunType::JITCompiled => {
                    let bytecode = self.compile_selection.bytecodes(provider_factory.clone(), Some(vec![tx_hash]))?;
                    let ctx = sim_utils::make_ext_ctx(run_type, bytecode, Some(&self.dir_path))?
                        .with_touch_tracking();
                    (ctx, false)
                }
                SimRunType::Native => {
                    (RevmcExtCtx::default().with_touch_tracking(), true)
                }
            };

        let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx).make_tx_sim(tx_hash)?;
        let (_result, elapsed) = bench_utils::time_fn(|| sim.run())?;
        
        bench_utils::check_tx_sim_validity(
            &provider_factory, 
            &mut sim, 
            vec![tx_hash], 
            is_native_exe, 
        )?;

        println!("Elapsed: {:?}", elapsed);

        Ok(())
    }

    pub fn run_block(
        &self,
        block_num: u64, 
        run_type: SimRunType, 
        block_chunk: Option<BlockPart>
    ) -> Result<()> {
        println!("BlockNum: {block_num:?}");

        let provider_factory = make_provider_factory(&self.reth_db_path)?;
        let mut block_txs = provider_factory.block(block_num.into())?
            .ok_or_eyre("block not found")?
            .body
            .iter()
            .map(|tx| tx.hash)
            .collect::<Vec<_>>();
        if let Some(block_chunk) = block_chunk {
            block_txs = block_chunk.split_txs(block_txs).0;
        }

        let is_native_exe = matches!(run_type, SimRunType::Native);
        let ext_ctx = 
            match run_type {
                SimRunType::AOTCompiled | SimRunType::JITCompiled => {
                    let bytecode = self.compile_selection.bytecodes(provider_factory.clone(), Some(block_txs.clone()))?;
                    sim_utils::make_ext_ctx(run_type, bytecode, Some(&self.dir_path))?
                        .with_touch_tracking()
                }
                SimRunType::Native => RevmcExtCtx::default()
                    .with_touch_tracking()
            };

        let mut sim = SimConfig::new(provider_factory.clone(), ext_ctx)
            .make_block_sim(block_num, block_chunk)?;
        let (_result, elapsed) = bench_utils::time_fn(|| sim.run())?;
        
        bench_utils::check_tx_sim_validity(
            &provider_factory, 
            &mut sim, 
            block_txs, 
            is_native_exe, 
        )?;

        println!("Elapsed: {:?}", elapsed);

        Ok(())
    }


}

impl<T, U> RunConfig<T, U> {

    pub fn run_call(
        &self,
        call: SimCall, 
        call_input: Bytes, 
        run_type: SimRunType, 
    ) -> Result<()> {
        println!("CallType: {call:?} with input: {call_input:?}");

        let bytecode = call.bytecode().original_bytes().into();
        println!("Bytecode: {}", hex::encode(&bytecode));
        let ext_ctx = sim_utils::make_ext_ctx(
            run_type, 
            vec![bytecode], 
            Some(&self.dir_path),
        )?;
        let mut sim = SimConfig::from(ext_ctx)
            .make_call_sim(call, call_input.clone())?;
        let (_result, elapsed) = bench_utils::time_fn(|| sim.run())?;

        println!("Elapsed: {:?}", elapsed);

        Ok(())
    }

}
