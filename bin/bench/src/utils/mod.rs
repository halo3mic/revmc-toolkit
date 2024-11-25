pub mod bench;
pub mod sim;

pub fn make_dir(dir_path: &std::path::PathBuf) -> eyre::Result<()> {
    if !dir_path.exists() {
        std::fs::create_dir_all(dir_path)?;
    }
    Ok(())
}

pub fn epoch_now() -> eyre::Result<u64> {
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    Ok(epoch)
}
