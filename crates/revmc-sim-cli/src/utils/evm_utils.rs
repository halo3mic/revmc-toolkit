use reth_chainspec::ChainSpecBuilder;
use reth_db::{self, DatabaseEnv};
use reth_provider::{
    providers::StaticFileProvider,
    ProviderFactory,
};
use revm::{Evm, db::CacheDB, primitives::{EnvWithHandlerCfg, B256}};
use std::path::Path;
use eyre::Result;

use revmc_sim_load::{ExternalContext, self as loader};


pub fn create_evm<ExtDB: revm::Database + revm::DatabaseRef>(
    dir_path: String,
    db: CacheDB<ExtDB>, 
    cfg_env: Option<EnvWithHandlerCfg>,
    codehash_select: Option<Vec<B256>>,
) -> Result<Evm<'static, ExternalContext, CacheDB<ExtDB>>> {
    let external_ctx = loader::build_external_context(dir_path, codehash_select)?;
    let evm = revm::Evm::builder()
        .with_db(db)
        .with_external_context(external_ctx);
    if let Some(cfg_env) = cfg_env {
        Ok(evm
            .with_env_with_handler_cfg(cfg_env)
            .append_handler_register(loader::register_handler)
            .build())
    } else {
        Ok(evm
            .append_handler_register(loader::register_handler)
            .build())
    }
}


pub fn make_provider_factory(db_path: &str) -> Result<ProviderFactory<DatabaseEnv>> {
    let db_path = Path::new(db_path);
    let db = reth_db::open_db_read_only(db_path.join("db").as_path(), Default::default())?;

    let spec = ChainSpecBuilder::mainnet().build();
    let stat_file_provider = StaticFileProvider::read_only(db_path.join("static_files"))?;
    let factory = ProviderFactory::new(db, spec.into(), stat_file_provider);

    Ok(factory)
}

