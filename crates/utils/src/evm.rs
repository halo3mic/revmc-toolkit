use eyre::Result;
use reth_chainspec::ChainSpecBuilder;
pub use reth_db::DatabaseEnv;
use reth_provider::providers::StaticFileProvider;
pub use reth_provider::ProviderFactory;
use std::path::Path;

pub fn make_provider_factory(db_path: &Path) -> Result<ProviderFactory<DatabaseEnv>> {
    let db = reth_db::open_db_read_only(db_path.join("db").as_path(), Default::default())?;

    let spec = ChainSpecBuilder::mainnet().build();
    let stat_file_provider = StaticFileProvider::read_only(db_path.join("static_files"))?;
    let factory = ProviderFactory::new(db, spec.into(), stat_file_provider);

    Ok(factory)
}
