use super::*;
use anyhow::{Context as _, ensure};
use tokio::io::{AsyncBufReadExt as _, BufReader};
#[tokio::test]
#[ignore = "requires the isolated Apache-generated HiveThriftPeer classpath; never connects to a saved host"]
async fn apache_generated_java_thrift_peer_verifies_plain_and_sasl_native_clients() -> Result<()> {
    let classpath = std::env::var("TOMATO_HIVE_JAVA_CLASSPATH")
        .context("set the verified loopback peer classpath")?;
    let mut modes = vec![
        "plain",
        "sasl",
        "http-basic",
        "http-basic-expiry",
        "http-basic-nocookie",
    ];
    if std::env::var("TOMATO_HIVE_TEST_KERBEROS").as_deref() == Ok("isolated-loopback-realm") {
        ensure!(cfg!(unix), "This fixture uses an isolated MIT ticket cache");
        modes.extend([
            "gss-auth",
            "gss-auth-int",
            "gss-auth-conf",
            "http-gss",
            "http-gss-expiry",
            "http-gss-nocookie",
        ]);
    }
    for mode in modes {
        let mut command = tokio::process::Command::new("java");
        if mode.starts_with("gss-") || mode.starts_with("http-gss") {
            let config = std::env::var("KRB5_CONFIG")?;
            let jaas = std::env::var("TOMATO_HIVE_JAVA_JAAS")?;
            command.args([
                format!("-Djava.security.krb5.conf={config}"),
                format!("-Djava.security.auth.login.config={jaas}"),
            ]);
        }
        command
            .args(["-cp", &classpath, "HiveThriftPeer", mode])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x0800_0000);
        let mut child = command.spawn()?;
        let mut stdout =
            BufReader::new(child.stdout.take().context("peer stdout missing")?).lines();
        let port = tokio::time::timeout(std::time::Duration::from_secs(15), stdout.next_line())
            .await??
            .context("peer did not start")?
            .parse::<u16>()?;
        ensure!(port > 0, "peer port missing");
        let mut connection = Connection::new("HIVE");
        connection.set("host", "127.0.0.1");
        connection.set("port", port);
        connection.set("username", "fixture-user");
        connection.set("password", "fixture-only");
        connection.set(
            "database",
            if mode.starts_with("http-gss") {
                format!("fixture_database;principal=hive/localhost@TOMATO.TEST;transportMode=http;httpPath=fixture;cookieAuth={}", mode != "http-gss-nocookie")
            } else if mode.starts_with("http-basic") {
                format!("fixture_database;transportMode=http;httpPath=fixture;cookieAuth={}", mode != "http-basic-nocookie")
            } else if let Some(qop) = mode.strip_prefix("gss-") {
                format!("fixture_database;principal=hive/localhost@TOMATO.TEST;sasl.qop={qop}")
            } else if mode == "plain" {
                "fixture_database;auth=noSasl".into()
            } else {
                "fixture_database".into()
            },
        );
        let before = serde_json::to_value(&connection)?;
        let mut databases = super::super::super::Databases::default();
        let result = databases
            .query(&connection, "SELECT 42, '中文', unhex('00FF'), NULL")
            .await?;
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].columns,
            vec!["id", "中文列", "payload", "missing"]
        );
        assert_eq!(result[0].rows[0][0], i64::MAX.to_string());
        assert_eq!(result[0].rows[0][1], "中文");
        assert_eq!(result[0].rows[0][2].to_uppercase(), "00FF");
        assert!(result[0].is_null(0, 3));
        assert_eq!(result[0].column_types, vec![-5, 12, -2, 12]);
        databases.disconnect(&connection.id).await?;
        let status =
            tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await??;
        ensure!(
            status.success(),
            "generated Java peer rejected the {mode} exchange"
        );
        assert_eq!(serde_json::to_value(&connection)?, before);
    }
    Ok(())
}
