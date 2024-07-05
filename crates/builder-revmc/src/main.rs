pub mod utils;
mod build;
mod sim;

use reth_db::open_db_read_only;
use reth_provider::{
    providers::StaticFileProvider,
    ProviderFactory, 
    StateProvider,
};
use reth_revm::database::StateProviderDatabase;
use revm::{Evm, Database};
use reth_primitives::{Address, Bytecode as RethBytecode, keccak256};
use reth_chainspec::ChainSpecBuilder;
use revm::primitives::{address, TransactTo, Bytes, U256, Env, Bytecode};
use revmc::EvmContext;

use std::{path::Path, str::FromStr};
use eyre::{OptionExt, Result};


fn main() -> Result<()> {
    dotenv::dotenv().ok();
    
    let db_path = std::env::var("RETH_DB_PATH")?;
    let state_provider = make_state_provider(&db_path)?;
    
    let univ2_router = address!("f164fC0Ec4E93095b804a4795bBe1e041497b92a");
    let label = "univ2_router";
    let code = state_provider.account_code(univ2_router)?
        .ok_or_eyre("No code found for address")?;

    // Compile
    println!("compiling 👀");
    build::compile(label, &code, None)?;
    println!("compiled 🔥");

    // Run
    let mut env = Env::default();
    env.tx.transact_to = TransactTo::Call(univ2_router);
    env.tx.data = Bytes::from_str("0xd06ca61f0000000000000000000000000000000000000000000000000de0b6b3a764000000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000006982508145454ce325ddbe47a25d4ec3d2311933")?;
    env.tx.gas_limit = 100_000;

    // run_a(label, env, code)?;
    run_b(label, env, state_provider)?;

    Ok(())
}


fn run_a(label: &str, env: Env, code: RethBytecode) -> Result<()> {
    let bytecode = revm::interpreter::analysis::to_analysed(Bytecode::new_raw(
        Bytes::copy_from_slice(code.bytes_slice()),
    ));
    let contract = revm_interpreter::Contract::new_env(&env, bytecode, None);
    let mut host = revm_interpreter::DummyHost::new(env);


    let f = build::load(&label)?;
    println!("loaded 🚀");
    let gas_limit = 100_000;
    let stack_input: Vec<U256> = vec![];

    let mut run = |f: revmc::EvmCompilerFn| {
        let mut interpreter = revm_interpreter::Interpreter::new(contract.clone(), gas_limit, false);
        host.clear();

        let (mut ecx, stack, stack_len) = EvmContext::from_interpreter_with_stack(&mut interpreter, &mut host);

        for (i, input) in stack_input.iter().enumerate() {
            stack.as_mut_slice()[i] = input.into();
        }
        *stack_len = stack_input.len();

        let r = unsafe { f.call_noinline(Some(stack), Some(stack_len), &mut ecx) };
        (r, interpreter.next_action)
    };


    let res = run(f);
    println!("Result: {:?}", res);

    Ok(())
}

fn run_b(label: &str, env: Env, state_provider: impl StateProvider + 'static) -> Result<()> {
    let spd = StateProviderDatabase::new(state_provider);
    let mut evm = utils::build_evm(spd);
    evm.context.evm.env = Box::new(env);

    let result = evm.transact()?;
    eprintln!("{:#?}", result.result);

    Ok(())
}

fn make_state_provider(db_path: &str) -> Result<impl StateProvider> {
    let db_path = Path::new(db_path);
    let db = open_db_read_only(db_path.join("db").as_path(), Default::default())?;

    let spec = ChainSpecBuilder::mainnet().build();
    let stat_file_provider = StaticFileProvider::read_only(db_path.join("static_files"))?;
    let factory = ProviderFactory::new(db, spec.into(), stat_file_provider);
    let state_provider = factory.latest()?;

    Ok(state_provider)
}

fn compile_contract(label: &str, provider: impl StateProvider, address: Address) -> Result<()> {
    let code = provider.account_code(address)?.ok_or_eyre("No code found for address")?;
    build::compile(label, &code, None)
}

// todo: impl as benches
fn native_sim() {}

fn aot_sim() {}

fn jit_sim() {}

