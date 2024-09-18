use revmc_toolbox_build::{CompilerOptions, CodeWithOptions};
use serde::{Deserialize, Deserializer};
use reth_provider::StateProvider;
use revm::primitives::Address;
use std::iter::IntoIterator;
use revm::primitives::B256;
use revmc::EvmCompilerFn;
use std::path::PathBuf;
use serde_json::Value;

use eyre::{OptionExt, Result};


// pub fn compile_aot_from_contracts<P>(
//     state_provider: &P,
//     contracts: &[Address],
//     fallback_opt: Option<CompilerOptions>,
// ) -> Result<Vec<Result<()>>> 
//     where
// {
//     compile_aot_from_contracts_with_fn(
//         |account| fetch_code_for_account(state_provider, account),
//         contracts,
//         fallback_opt,
//     )
// }

pub fn compile_aot_from_contracts_with_fn<F>(
    account_to_code_fn: F,
    contracts: &[Address],
    fallback_opt: Option<CompilerOptions>,
) -> Result<Vec<Result<()>>> 
where F: Fn(Address) -> Result<Vec<u8>> {
    let contracts = contracts.iter().map(|&account| {
        let code = account_to_code_fn(account)?;
        Ok(CodeWithOptions { code, options: None })
    }).collect::<Result<Vec<_>>>()?;
    revmc_toolbox_build::compile_contracts_aot(contracts, fallback_opt)
}

fn fetch_code_for_account(state_provider: &impl StateProvider, account: Address) -> Result<Vec<u8>> {
    let code = state_provider.account_code(account)?
        .ok_or_eyre("No code found for address")?;
    let code_bytes = code.original_byte_slice().to_vec();
    Ok(code_bytes)
}
