use reth_chainspec::ChainSpecBuilder;
use reth_db::{self, DatabaseEnv};
use reth_provider::{
    providers::StaticFileProvider,
    ProviderFactory,
};
use std::path::Path;
use eyre::Result;


pub fn make_provider_factory(db_path: &str) -> Result<ProviderFactory<DatabaseEnv>> {
    let db_path = Path::new(db_path);
    let db = reth_db::open_db_read_only(db_path.join("db").as_path(), Default::default())?;

    let spec = ChainSpecBuilder::mainnet().build();
    let stat_file_provider = StaticFileProvider::read_only(db_path.join("static_files"))?;
    let factory = ProviderFactory::new(db, spec.into(), stat_file_provider);

    Ok(factory)
}