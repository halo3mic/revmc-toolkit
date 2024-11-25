pub fn make_dir(dir_path: &std::path::Path) -> eyre::Result<()> {
    if !dir_path.exists() {
        std::fs::create_dir_all(dir_path)?;
    }
    Ok(())
}