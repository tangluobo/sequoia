use super::*;
use std::io::Read;

fn authorization() -> Result<Authorization> {
    let mut password = Password::default();
    password.edit().push_str("fixture-only-secret");
    password.into_authorization(Control::default())
}
fn child(mode: &str, path: &Path) -> Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args([
            "--exact",
            "hosts::elevation::tests::sudo_process_child",
            "--ignored",
            "--nocapture",
        ])
        .env("TOMATO_OWNED_HOSTS_CHILD", mode)
        .env("TOMATO_OWNED_HOSTS_FILE", path);
    Ok(command)
}
#[test]
#[ignore = "Owned subprocess helper, invoked by the adapter workflow tests"]
fn sudo_process_child() -> Result<()> {
    let Ok(mode) = std::env::var("TOMATO_OWNED_HOSTS_CHILD") else {
        return Ok(());
    };
    let mut password = String::new();
    std::io::stdin().read_to_string(&mut password)?;
    assert_eq!(password, "fixture-only-secret\n");
    assert!(!std::env::args().any(|arg| arg.contains("fixture-only-secret")));
    if mode == "reject" {
        std::process::exit(1);
    }
    if mode == "hang" {
        std::thread::sleep(Duration::from_secs(5));
    }
    std::fs::write(
        std::env::var_os("TOMATO_OWNED_HOSTS_FILE").unwrap(),
        "saved 中文",
    )?;
    Ok(())
}
#[test]
fn transient_password_process_success_rejection_and_json_redaction() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("hosts");
    let authorization = authorization()?;
    run(
        child("success", &path)?,
        &authorization,
        Duration::from_secs(5),
    )?;
    assert_eq!(std::fs::read_to_string(&path)?, "saved 中文");
    std::fs::remove_file(&path)?;
    let error = run(
        child("reject", &path)?,
        &authorization,
        Duration::from_secs(5),
    )
    .unwrap_err();
    assert!(error.to_string().contains("密码错误或权限不足"));
    assert!(!path.exists());
    let action = super::super::Action::SaveElevated {
        request: Box::new(super::super::SaveRequest {
            previous: Box::new(super::super::Snapshot {
                config: Default::default(),
                system: String::new(),
                config_stamp: None,
                system_stamp: None,
            }),
            config: Default::default(),
            system: Some("127.0.0.2 owned.test".into()),
            apply: false,
        }),
        authorization: Some(authorization),
    };
    assert!(!format!("{action:?}").contains("fixture-only-secret"));
    let encoded = serde_json::to_string(&action)?;
    assert!(!encoded.contains("fixture-only-secret") && !encoded.contains("authorization"));
    let super::super::Action::SaveElevated { authorization, .. } = serde_json::from_str(&encoded)?
    else {
        panic!()
    };
    assert!(authorization.is_none());
    Ok(())
}
#[test]
fn sudo_process_timeout_and_cancel_reap_child_before_return() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("must-not-exist");
    let authorization = authorization()?;
    let start = Instant::now();
    let error = run(
        child("hang", &path)?,
        &authorization,
        Duration::from_millis(150),
    )
    .unwrap_err();
    assert!(error.to_string().contains("超时"));
    assert!(start.elapsed() < Duration::from_secs(3));
    let control = authorization.control.clone();
    let cancel = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        control.cancel();
    });
    let error = run(
        child("hang", &path)?,
        &authorization,
        Duration::from_secs(10),
    )
    .unwrap_err();
    cancel.join().unwrap();
    assert!(error.to_string().contains("已取消"));
    assert!(start.elapsed() < Duration::from_secs(3));
    assert!(!path.exists());
    Ok(())
}
