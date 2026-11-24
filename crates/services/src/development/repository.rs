use anyhow::{Context, Result, ensure};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use tomato_core::development::Config;
pub const LIMIT: usize = 2 * 1024 * 1024;
pub fn digest(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .encode(ring::digest::digest(&ring::digest::SHA256, bytes))
}
fn bytes(path: &Path) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= LIMIT, "开发配置超过 2 MiB");
    Ok(Some(bytes))
}
pub fn load(path: &Path) -> Result<(Config, Option<String>)> {
    let Some(bytes) = bytes(path)? else {
        return Ok((Config::default(), None));
    };
    let config = Config::parse(&bytes).context("无法解析原版 development.json，原文件未修改")?;
    Ok((config, Some(digest(&bytes))))
}
pub fn save(path: &Path, config: &Config, revision: Option<&str>) -> Result<String> {
    ensure!(
        config.projects.len() <= 1000
            && config.runtimes.len() <= 1000
            && config.build_tools.len() <= 1000,
        "开发配置项目过多"
    );
    let parent = path.parent().context("开发配置缺少父目录")?;
    fs::create_dir_all(parent)?;
    let lock_path = PathBuf::from(format!("{}.rust.lock", path.display()));
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    lock.try_lock().context("开发配置正被其他程序修改")?;
    let old = bytes(path)?;
    ensure!(
        old.as_deref().map(digest).as_deref() == revision,
        "开发配置已被其他程序修改，未覆盖原文件"
    );
    let encoded = serde_json::to_vec_pretty(config)?;
    ensure!(encoded.len() <= LIMIT, "开发配置超过 2 MiB");
    if let Some(old) = &old {
        let backup = PathBuf::from(format!("{}.before-rust.bak", path.display()));
        if !backup.exists() {
            use std::io::Write;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(old)?;
            file.as_file().sync_all()?;
            file.persist_noclobber(backup).map_err(|e| e.error)?;
        }
    }
    ensure!(bytes(path)? == old, "开发配置已发生外部修改，保存已取消");
    tomato_core::config::atomic_write(path, &encoded)?;
    Ok(digest(&encoded))
}
