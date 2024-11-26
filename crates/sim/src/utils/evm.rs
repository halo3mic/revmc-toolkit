use reth_evm::ConfigureEvmEnv;
use reth_evm_ethereum::EthEvmConfig;
use reth_primitives::Header;
use revm::primitives::{BlockEnv, CfgEnv, CfgEnvWithHandlerCfg, EnvWithHandlerCfg, SpecId, TxEnv};
use revm::{db::CacheDB, handler::register::HandleRegister, Database, DatabaseRef, Evm};

pub(crate) fn make_evm<'a, ExtCtx, DBInner: Database + DatabaseRef>(
    db: CacheDB<DBInner>,
    ext_ctx: ExtCtx,
    handler_register: Option<HandleRegister<ExtCtx, CacheDB<DBInner>>>,
    env: Option<EnvWithHandlerCfg>,
) -> Evm<'a, ExtCtx, CacheDB<DBInner>> {
    let builder = revm::Evm::builder()
        .with_db(db)
        .with_external_context(ext_ctx)
        .with_env_with_handler_cfg(env.unwrap_or_default());

    if let Some(handler_register) = handler_register {
        builder.append_handler_register(handler_register).build()
    } else {
        builder.build()
    }
}

pub(crate) fn env_with_handler_cfg(chain_id: u64, block_header: &Header) -> EnvWithHandlerCfg {
    let mut block_env = block_env_from_block(block_header);
    block_env.prevrandao = Some(block_header.mix_hash);
    let cfg = CfgEnv::default().with_chain_id(chain_id);
    let cfg_env = CfgEnvWithHandlerCfg::new_with_spec_id(cfg, SpecId::CANCUN);
    EnvWithHandlerCfg::new_with_cfg_env(cfg_env, block_env, TxEnv::default())
}

// todo: Fill block env in simpler way with less imports
pub(crate) fn block_env_from_block(block_header: &Header) -> BlockEnv {
    let mut block_env = BlockEnv::default();
    let eth_evm_cfg = EthEvmConfig::default();
    eth_evm_cfg.fill_block_env(
        &mut block_env,
        block_header,
        block_header.number >= 15537393,
    );
    block_env
}
