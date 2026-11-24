use anyhow::Result;
use tomato_core::Connection;
use tomato_services::redis_data::{self, Action, Output};

#[tokio::test]
#[ignore = "Requires the owned loopback Linux Redis cluster and fixture SSH key; never uses saved hosts"]
async fn real_cluster_ssh_discovery_all_types_primary_scan_and_configuration_readonly() -> Result<()>
{
    use anyhow::{Context as _, ensure};
    ensure!(
        std::env::var("TOMATO_TEST_REDIS_CLUSTER_OWNED").as_deref() == Ok("owned-loopback-linux"),
        "Owned fixture required"
    );
    let profile = std::path::PathBuf::from(
        std::env::var_os("TOMATO_DATA_DIR").context("Owned profile missing")?,
    );
    ensure!(
        profile.is_absolute()
            && profile
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("redis-1692-")),
        "Owned profile required"
    );
    let configuration = tomato_core::Store::default_path();
    ensure!(!configuration.exists(), "Fixture configuration must be new");
    let key = std::path::PathBuf::from(
        std::env::var_os("TOMATO_TEST_LINUX_KEY").context("Fixture key missing")?,
    );
    ensure!(
        key.is_file() && key.file_name().is_some_and(|n| n == "fixture-ed25519"),
        "Owned key required"
    );
    let mut jump = Connection::new("SSH");
    jump.set("host", "127.0.0.1");
    jump.set(
        "port",
        std::env::var("TOMATO_TEST_LINUX_SSH_PORT")?.parse::<u16>()?,
    );
    jump.set("username", "tomato");
    jump.set("usePassword", false);
    jump.set("useKey", true);
    jump.set("privateKeyPaths", serde_json::json!([key]));
    tomato_core::Store {
        connections: vec![jump.clone()],
        ..Default::default()
    }
    .save(&configuration)?;
    let bytes = std::fs::read(&configuration)?;
    let mut c = Connection::new("REDIS");
    c.set("redisCluster", true);
    c.set(
        "redisClusterNodes",
        "127.0.0.1:17301,127.0.0.1:17302,127.0.0.1:17303",
    );
    c.set("password", "OwnedRedisFixture1692");
    c.set("useSshTunnel", true);
    c.set("sshTunnelHostId", jump.id);
    c.set("futureFixtureField", serde_json::json!({"preserve":true}));
    let before = serde_json::to_value(&c)?;
    assert!(matches!(run(&c,Action::Databases).await?,Output::Databases(v)if v==[0]));
    let prefix = format!("owned-1692-{}", uuid::Uuid::new_v4().simple());
    for (kind, text) in [
        ("string", "中文 O'Reilly"),
        ("list", "一\n二\n"),
        ("set", "a\nb\n"),
        ("hash", "a 中文\nb value\n"),
        ("zset", "1 first\n2 second\n"),
    ] {
        let name = format!("{prefix}-{kind}");
        run(
            &c,
            Action::Write {
                key: name.clone(),
                kind: kind.into(),
                text: text.into(),
                create: true,
            },
        )
        .await?;
        let (actual, readback, _) = value(&c, &name).await?;
        ensure!(actual == kind, "Cluster type readback failed");
        if matches!(kind, "string" | "list") {
            ensure!(
                readback == text.trim_end_matches('\n'),
                "Cluster value readback failed"
            );
        } else {
            let actual: std::collections::BTreeSet<_> = readback.lines().collect();
            let expected: std::collections::BTreeSet<_> = text.lines().collect();
            ensure!(actual == expected, "Cluster collection readback failed");
        }
        run(
            &c,
            Action::Ttl {
                key: name.clone(),
                seconds: 600,
            },
        )
        .await?;
        ensure!(
            (1..=600).contains(&value(&c, &name).await?.2),
            "Cluster TTL readback failed"
        );
    }
    let pong = redis_data::raw_command(&c, &["PING".into()]).await?;
    ensure!(pong.contains("PONG"), "Raw cluster command was not routed");
    let Output::Keys { keys, .. } = run(
        &c,
        Action::Scan {
            pattern: prefix.clone(),
            cursor: 0,
        },
    )
    .await?
    else {
        anyhow::bail!("Missing complete primary scan")
    };
    ensure!(keys.len() == 5, "Cluster scan missed another primary");
    run(&c, Action::Delete { keys: keys.clone() }).await?;
    let Output::Keys { keys, .. } = run(
        &c,
        Action::Scan {
            pattern: prefix,
            cursor: 0,
        },
    )
    .await?
    else {
        anyhow::bail!("Missing final scan")
    };
    ensure!(keys.is_empty(), "Multi-slot delete did not remove all keys");
    ensure!(
        std::fs::read(&configuration)? == bytes && serde_json::to_value(c)? == before,
        "Original connection/configuration changed"
    );
    Ok(())
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn run(c: &Connection, action: Action) -> Result<Output> {
    redis_data::execute(c, action).await
}
async fn value(c: &Connection, key: &str) -> Result<(String, String, i64)> {
    match run(c, Action::Read { key: key.into() }).await? {
        Output::Value {
            kind, text, ttl, ..
        } => Ok((kind, text, ttl)),
        _ => unreachable!(),
    }
}
#[tokio::test]
#[ignore = "Set TOMATO_TEST_REDIS_SERVER to a local redis-server executable; starts its own isolated loopback child"]
async fn real_redis_types_ttl_conflicts_and_database_isolation() -> Result<()> {
    let executable =
        std::env::var_os("TOMATO_TEST_REDIS_SERVER").expect("TOMATO_TEST_REDIS_SERVER required");
    let dir = tempfile::tempdir()?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let mut cmd = std::process::Command::new(executable);
    cmd.current_dir(dir.path())
        .args([
            "--bind",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--save",
            "",
            "--appendonly",
            "no",
            "--maxmemory",
            "32mb",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let _child = Child(cmd.spawn()?);
    let mut c = Connection::new("REDIS");
    c.set("host", "127.0.0.1");
    c.set("port", port);
    c.set("redisDatabase", 3);
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    assert!(
        matches!(run(&c,Action::Databases).await?,Output::Databases(databases) if databases==(0..16).collect::<Vec<_>>())
    );
    for (kind, text) in [
        ("string", "中文 value\nline2"),
        ("list", "one\n中文 value"),
        ("set", "one\ntwo"),
        ("hash", "field value with spaces\n中文 空 值"),
        ("zset", "1.5 one\n-2 中文 value"),
    ] {
        let key = format!("namespace:{kind}");
        run(
            &c,
            Action::Write {
                key: key.clone(),
                kind: kind.into(),
                text: text.into(),
                create: true,
            },
        )
        .await?;
        let (actual, read, ttl) = value(&c, &key).await?;
        assert_eq!(actual, kind);
        assert_eq!(ttl, -1);
        if kind == "string" || kind == "list" || kind == "hash" {
            assert_eq!(read, text);
        }
        assert!(
            run(
                &c,
                Action::Write {
                    key: key.clone(),
                    kind: "string".into(),
                    text: "overwrite".into(),
                    create: true
                }
            )
            .await
            .is_err()
        );
        run(
            &c,
            Action::Ttl {
                key: key.clone(),
                seconds: 120,
            },
        )
        .await?;
        run(
            &c,
            Action::Write {
                key: key.clone(),
                kind: kind.into(),
                text: text.into(),
                create: false,
            },
        )
        .await?;
        let (_, _, ttl) = value(&c, &key).await?;
        assert!((110..=120).contains(&ttl));
    }
    let before = value(&c, "namespace:zset").await?;
    assert!(
        run(
            &c,
            Action::Write {
                key: "namespace:zset".into(),
                kind: "zset".into(),
                text: "NaN corrupt".into(),
                create: false
            }
        )
        .await
        .is_err()
    );
    assert_eq!(value(&c, "namespace:zset").await?.1, before.1);
    run(
        &c,
        Action::Write {
            key: "rename-target".into(),
            kind: "string".into(),
            text: "old".into(),
            create: true,
        },
    )
    .await?;
    run(
        &c,
        Action::Rename {
            key: "namespace:list".into(),
            name: "rename-target".into(),
        },
    )
    .await?;
    assert_eq!(value(&c, "rename-target").await?.0, "list");
    run(
        &c,
        Action::Rename {
            key: "rename-target".into(),
            name: "renamed".into(),
        },
    )
    .await?;
    assert_eq!(value(&c, "renamed").await?.0, "list");
    let Output::Keys { keys, .. } = run(
        &c,
        Action::Scan {
            pattern: "namespace:*".into(),
            cursor: 0,
        },
    )
    .await?
    else {
        unreachable!()
    };
    assert_eq!(keys.len(), 4);
    let mut other = c.clone();
    other.set("redisDatabase", 4);
    assert_eq!(value(&other, "renamed").await?.0, "none");
    run(
        &c,
        Action::Ttl {
            key: "renamed".into(),
            seconds: -1,
        },
    )
    .await?;
    assert_eq!(value(&c, "renamed").await?.2, -1);
    run(
        &c,
        Action::Delete {
            keys: vec!["renamed".into(), "namespace:hash".into()],
        },
    )
    .await?;
    assert_eq!(value(&c, "renamed").await?.0, "none");
    Ok(())
}

#[test]
fn invalid_scores_are_rejected_before_writes() {
    assert!(redis_data::write_args("zset", "NaN member").is_err());
    assert!(redis_data::write_args("zset", "inf member").is_err());
    assert!(redis_data::write_args("stream", "anything").is_err());
    assert_eq!(
        redis_data::write_args("hash", "field value with spaces").unwrap(),
        ["field", "value with spaces"]
    );
}

#[tokio::test]
#[ignore = "Set TOMATO_TEST_REDIS_SERVER and TOMATO_TEST_REDIS_CLI; starts three isolated loopback Redis Cluster nodes"]
async fn real_cluster_routes_types_and_scans_all_primaries() -> Result<()> {
    let server = std::env::var_os("TOMATO_TEST_REDIS_SERVER").expect("server executable required");
    let cli = std::env::var_os("TOMATO_TEST_REDIS_CLI").expect("CLI executable required");
    let dir = tempfile::tempdir()?;
    let mut base = 0u16;
    for candidate in (24000..30000u16).step_by(7) {
        let sockets: std::io::Result<Vec<_>> = [
            candidate,
            candidate + 1,
            candidate + 2,
            candidate + 10000,
            candidate + 10001,
            candidate + 10002,
        ]
        .into_iter()
        .map(|p| std::net::TcpListener::bind(("127.0.0.1", p)))
        .collect();
        if sockets.is_ok() {
            base = candidate;
            break;
        }
    }
    anyhow::ensure!(base > 0, "could not reserve isolated cluster ports");
    let ports = [base, base + 1, base + 2];
    let mut children = Vec::new();
    for port in ports {
        let root = dir.path().join(port.to_string());
        std::fs::create_dir(&root)?;
        let mut command = std::process::Command::new(&server);
        command
            .current_dir(&root)
            .args([
                "--bind",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--save",
                "",
                "--appendonly",
                "no",
                "--cluster-enabled",
                "yes",
                "--cluster-config-file",
                "nodes.conf",
                "--cluster-node-timeout",
                "5000",
                "--maxmemory",
                "32mb",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        children.push(Child(command.spawn()?));
    }
    for port in ports {
        for _ in 0..100 {
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
    }
    let mut command = std::process::Command::new(cli);
    command.arg("--cluster").arg("create");
    for port in ports {
        command.arg(format!("127.0.0.1:{port}"));
    }
    command.args(["--cluster-replicas", "0", "--cluster-yes"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output()?;
    anyhow::ensure!(
        output.status.success(),
        "cluster create failed: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let mut c = Connection::new("REDIS");
    c.set("redisCluster", true);
    c.set("redisClusterNodes", format!("127.0.0.1:{base}"));
    for _ in 0..100 {
        if run(
            &c,
            Action::Scan {
                pattern: "*".into(),
                cursor: 0,
            },
        )
        .await
        .is_ok()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        matches!(run(&c,Action::Databases).await?,Output::Databases(databases) if databases==[0])
    );
    for index in 0..12 {
        run(
            &c,
            Action::Write {
                key: format!("cluster:{index}"),
                kind: "string".into(),
                text: format!("中文 {index}"),
                create: true,
            },
        )
        .await?;
    }
    let Output::Keys { keys, cursor } = run(
        &c,
        Action::Scan {
            pattern: "cluster:*".into(),
            cursor: 0,
        },
    )
    .await?
    else {
        unreachable!()
    };
    assert_eq!(cursor, 0);
    assert_eq!(keys.len(), 12);
    for index in 0..12 {
        assert_eq!(
            value(&c, &format!("cluster:{index}")).await?.1,
            format!("中文 {index}")
        );
    }
    run(
        &c,
        Action::Write {
            key: "{fixture}:hash".into(),
            kind: "hash".into(),
            text: "field 中文 value".into(),
            create: true,
        },
    )
    .await?;
    assert_eq!(value(&c, "{fixture}:hash").await?.1, "field 中文 value");
    run(
        &c,
        Action::Rename {
            key: "{fixture}:hash".into(),
            name: "{fixture}:renamed".into(),
        },
    )
    .await?;
    run(
        &c,
        Action::Delete {
            keys: vec![
                "cluster:1".into(),
                "cluster:2".into(),
                "{fixture}:renamed".into(),
            ],
        },
    )
    .await?;
    assert_eq!(value(&c, "cluster:1").await?.0, "none");
    assert_eq!(value(&c, "cluster:2").await?.0, "none");
    drop(children);
    Ok(())
}
