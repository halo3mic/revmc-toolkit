mod utils;
mod build;
mod evm;
mod sim;

use reth_revm::database::StateProviderDatabase;
use revm::primitives::{address, Bytes, TransactTo};

use std::str::FromStr;
use eyre::{Result, OptionExt};

// todo: rename to revmc-sim
// todo: compilation in a seperate crate + cli + json parsing of settings
// todo: Tx simulation
// todo: Block simulation
// todo: benchmarking + set different compile options / tx_bench & block_bench

fn main() -> Result<()>{
    dotenv::dotenv().ok();
    
    let db_path = std::env::var("RETH_DB_PATH")?;
    let provider_factory = utils::make_provider_factory(&db_path)?;

    // let contracts = vec![
    //     CompileArgs2 {
    //         address: address!("f164fC0Ec4E93095b804a4795bBe1e041497b92a"),
    //         label: String::from("univ2_router"),
    //         options: None,
    //     },
    //     CompileArgs2 {
    //         address: address!("1111111254eeb25477b68fb85ed929f73a960582"),
    //         label: String::from("1inch_v5"),
    //         options: None,
    //     },
    //     CompileArgs2 {
    //         address: address!("87870bca3f3fd6335c3f4ce8392d69350b4fa4e2"),
    //         label: String::from("aave_v3"),
    //         options: None,
    //     },
    // ];
    // compile_contracts2(state_provider, contracts)?;

    
    // Run
    
    let univ2_router = address!("f164fC0Ec4E93095b804a4795bBe1e041497b92a");
    let label = "univ2_router";
    let state_provider = provider_factory.latest()?;
    let code = state_provider.account_code(univ2_router)?
        .ok_or_eyre("No code found for address")?;
    let spdb = StateProviderDatabase::new(state_provider);

    let mut evm = evm::create_evm(vec![
        (label.to_string(), code.hash_slow()).into()
    ], spdb)?;

    evm.context.evm.env.tx.transact_to = TransactTo::Call(univ2_router);
    evm.context.evm.env.tx.data = Bytes::from_str("0xd06ca61f0000000000000000000000000000000000000000000000000de0b6b3a764000000000000000000000000000000000000000000000000000000000000000000400000000000000000000000000000000000000000000000000000000000000002000000000000000000000000c02aaa39b223fe8d0a0e5c4f27ead9083c756cc20000000000000000000000006982508145454ce325ddbe47a25d4ec3d2311933")?;
    evm.context.evm.env.tx.gas_limit = 100_000;

    match evm.transact() {
        Ok(res) =>  eprintln!("{:#?}", res.result),
        Err(e) => println!("error: {:?}", e),
    }

    Ok(())
}



