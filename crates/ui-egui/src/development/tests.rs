use super::*;
#[test]
fn lazy_development_load_does_not_create_or_rewrite_legacy_config() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("development.json");
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut state = State::new(&runtime, &ctx, path.clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    while !state.loaded && Instant::now() < deadline {
        state.poll(&ctx);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(state.loaded);
    assert!(state.config.projects.is_empty());
    assert!(!path.exists());
    Ok(())
}
