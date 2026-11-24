use super::*;

fn fixture() -> Result<(tempfile::TempDir, PathBuf, PathBuf, SaveRequest)> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("hosts_groups.json");
    let system = dir.path().join("hosts");
    fs::write(&config, r#"{"groups":[],"future":true}"#)?;
    fs::write(&system, "127.0.0.1 localhost\n")?;
    let previous = Box::new(load(&config, &system)?);
    let request = SaveRequest {
        config: previous.config.clone(),
        previous,
        system: Some("127.0.0.2 fixture.test\n".into()),
        apply: false,
    };
    Ok((dir, config, system, request))
}
#[test]
fn permission_challenge_preserves_original_files_and_retry_rechecks_revision() -> Result<()> {
    let (_dir, config, system, request) = fixture()?;
    let old_config = fs::read(&config)?;
    let old_system = fs::read(&system)?;
    let error = save_using(
        &config,
        &system,
        request,
        true,
        |_| Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied).into()),
        |_| panic!("a denied open must not invoke rollback"),
    )
    .unwrap_err();
    let challenge = error.downcast::<ElevationRequired>().unwrap();
    assert_eq!(fs::read(&config)?, old_config);
    assert_eq!(fs::read(&system)?, old_system);
    let external = "127.0.0.3 external.test";
    fs::write(&system, external)?;
    let error = save_using(
        &config,
        &system,
        *challenge.0,
        false,
        |_| panic!("stale authorization must not write"),
        |_| panic!(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("系统 Hosts 已被其他程序修改"));
    assert_eq!(fs::read_to_string(&system)?, external);
    assert_eq!(fs::read(&config)?, old_config);
    Ok(())
}
#[test]
fn retry_writes_compact_legacy_config_and_config_conflict_restores_hosts() -> Result<()> {
    let (_dir, config, system, request) = fixture()?;
    let original = request.previous.system.clone();
    let desired = request.system.clone().unwrap();
    let external = r#"{"groups":[],"future":"external"}"#;
    let error = save_using(
        &config,
        &system,
        request.clone(),
        false,
        |text| {
            fs::write(&system, text)?;
            fs::write(&config, external)?;
            Ok(())
        },
        |text| Ok(fs::write(&system, text)?),
    )
    .unwrap_err();
    assert!(error.to_string().contains("Hosts 分组已被其他程序修改"));
    assert_eq!(fs::read_to_string(&config)?, external);
    assert_eq!(fs::read_to_string(&system)?, original);
    let previous = Box::new(load(&config, &system)?);
    let request = SaveRequest {
        config: previous.config.clone(),
        previous,
        ..request
    };
    let next = save_using(
        &config,
        &system,
        request,
        false,
        |text| Ok(fs::write(&system, text)?),
        |_| panic!(),
    )?;
    assert_eq!(next.system, desired);
    assert_eq!(next.config.extra["future"], "external");
    assert!(fs::read_to_string(&config)?.contains("\"groups\":["));
    Ok(())
}
#[test]
fn partial_write_failure_rolls_back_without_publishing_config() -> Result<()> {
    let (_dir, config, system, request) = fixture()?;
    let old = fs::read(&config)?;
    let original = request.previous.system.clone();
    let error = save_using(
        &config,
        &system,
        request,
        false,
        |_| {
            fs::write(&system, "partial")?;
            anyhow::bail!("owned injected write interruption")
        },
        |text| Ok(fs::write(&system, text)?),
    )
    .unwrap_err();
    assert!(error.to_string().contains("owned injected"));
    assert_eq!(fs::read(&config)?, old);
    assert_eq!(fs::read_to_string(&system)?, original);
    Ok(())
}
