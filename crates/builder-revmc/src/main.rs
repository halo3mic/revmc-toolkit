pub mod utils;
mod build;
mod sim;

use build::CompilerOptions;
use reth_db::open_db_read_only;
use reth_provider::{
    providers::StaticFileProvider,
    ProviderFactory, 
    StateProvider,
};
use reth_revm::database::StateProviderDatabase;
use revm::{Evm, Database, handler::register::EvmHandler};
use reth_primitives::{Address, Bytecode as RethBytecode, B256};
use reth_chainspec::ChainSpecBuilder;
use revm::primitives::{address, Bytes, FixedBytes, TransactTo};

use std::collections::HashMap;
use std::sync::Arc;
use std::{path::Path, str::FromStr};
use eyre::{OptionExt, Result};


fn main() -> Result<()>{
    dotenv::dotenv().ok();
    
    let db_path = std::env::var("RETH_DB_PATH")?;
    let state_provider = make_state_provider(&db_path)?;

    let contracts = vec![
        CompileArgs2 {
            address: address!("f164fC0Ec4E93095b804a4795bBe1e041497b92a"),
            label: String::from("univ2_router"),
            options: None,
        },
        CompileArgs2 {
            address: address!("1111111254eeb25477b68fb85ed929f73a960582"),
            label: String::from("1inch_v5"),
            options: None,
        },
        CompileArgs2 {
            address: address!("87870bca3f3fd6335c3f4ce8392d69350b4fa4e2"),
            label: String::from("aave_v3"),
            options: None,
        },
    ];
    compile_contracts2(state_provider, contracts)?;
    
    
    std::process::exit(0);
    
    // Run
    
    let label = "univ2_router";
    let code = state_provider.account_code(univ2_router)?
        .ok_or_eyre("No code found for address")?;
    let spdb = StateProviderDatabase::new(state_provider);

    let mut evm = create_evm(vec![SourceCode {
        label: label.to_string(),
        codehash: code.hash_slow(),
    }], spdb)?;

    evm.context.evm.env.tx.transact_to = TransactTo::Call(univ2_router);
    evm.context.evm.env.tx.data = Bytes::from_str("0xd06ca61f0000000000000000000000000000000000000000000000000de0b6b3a764000000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000006982508145454ce325ddbe47a25d4ec3d2311933")?;
    evm.context.evm.env.tx.gas_limit = 100_000;

    match evm.transact() {
        Ok(res) =>  eprintln!("{:#?}", res.result),
        Err(e) => println!("error: {:?}", e),
    }

    Ok(())
}

/// Compilation utils

use std::iter::IntoIterator;

struct CompileArgs2 {
    address: Address,
    label: String,
    options: Option<CompilerOptions>,
}

impl CompileArgs2 {

    fn into_compile_args(self, state_provider: &impl StateProvider) -> Result<CompileArgs> {
        let code = state_provider.account_code(self.address)?
            .ok_or_eyre("No code found for address")?;
        Ok(CompileArgs {
            label: self.label,
            code,
            options: self.options,
        })
    }

}

fn compile_contracts2(
    state_provider: Arc<impl StateProvider>,
    contracts: impl IntoIterator<Item=CompileArgs2>
) -> Result<Vec<Result<()>>> {
    let contracts = contracts.into_iter()
        .map(|c| c.into_compile_args(&state_provider))
        .collect::<Result<Vec<_>>>()?;
    let results = compile_contracts(contracts);
    Ok(results)
}

/// Compilation & Utils - Library

use rayon::prelude::*;

struct CompileArgs {
    label: String,
    code: RethBytecode,
    options: Option<CompilerOptions>,
}

fn compile_contracts(args: Vec<CompileArgs>) -> Vec<Result<()>> {
    args.into_par_iter()
        .map(|arg| compile_contract(arg))
        .collect()
}

fn compile_contract(arg: CompileArgs) -> Result<()> {
    build::compile(&arg.label, &arg.code, arg.options)
}

// Simulation utils

use libloading::Library;

// todo: can label just be the codehash?
struct SourceCode {
    label: String,
    codehash: FixedBytes<32>,
}

fn create_evm<E>(
    src_codes: Vec<SourceCode>, 
    db: impl Database<Error=E> + 'static
) -> Result<Evm<'static, ExternalContext, impl Database<Error=E>>> 
where E: std::fmt::Debug + 'static
{
    let mut external_ctx = ExternalContext::default();
    for src_code in src_codes {
        let fnc  = build::load(&src_code.label)?;
        external_ctx.add(src_code.codehash, fnc);
    }
    let evm = revm::Evm::builder()
        .with_db(db)
        .with_external_context(external_ctx)
        .append_handler_register(register_handler)
        .build();
    Ok(evm)
}

#[derive(Default)]
pub struct ExternalContext(HashMap<B256, (revmc::EvmCompilerFn, Library)>);  // todo: consider fast hashmap

impl ExternalContext {
    fn add(&mut self, code_hash: B256, fnc: (revmc::EvmCompilerFn, Library)) {
        self.0.insert(code_hash, fnc);
    }

    fn get_function(&self, bytecode_hash: B256) -> Option<revmc::EvmCompilerFn> {
        self.0.get(&bytecode_hash).map(|f| f.0)
    }
}

// This `+ 'static` bound is only necessary here because of an internal cfg feature.
fn register_handler<DB: Database + 'static>(handler: &mut EvmHandler<'_, ExternalContext, DB>) {
    let prev = handler.execution.execute_frame.clone();
    handler.execution.execute_frame = Arc::new(move |frame, memory, tables, context| {
        let interpreter = frame.interpreter_mut();
        let bytecode_hash = interpreter.contract.hash.unwrap_or_default();
        if let Some(f) = context.external.get_function(bytecode_hash) {
            Ok(unsafe { f.call_with_interpreter_and_memory(interpreter, memory, context) })
        } else {
            prev(frame, memory, tables, context)
        }
    });
}

// Benchmarking utils

fn make_state_provider(db_path: &str) -> Result<Arc<impl StateProvider>> {
    let db_path = Path::new(db_path);
    let db = open_db_read_only(db_path.join("db").as_path(), Default::default())?;

    let spec = ChainSpecBuilder::mainnet().build();
    let stat_file_provider = StaticFileProvider::read_only(db_path.join("static_files"))?;
    let factory = ProviderFactory::new(db, spec.into(), stat_file_provider);
    let state_provider = factory.latest()?;

    Ok(Arc::new(state_provider))
}


