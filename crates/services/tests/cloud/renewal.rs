use super::*;
use anyhow::{Context, ensure};
use chrono::Datelike;
use russh::{
    Channel, ChannelId,
    keys::{PrivateKey, ssh_key::private::Ed25519Keypair},
    server,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tomato_core::{Connection, Store};
use tomato_services::cloud::certificate::renewal::{self, Deployment, Options, Runner};

#[path = "../support/sftp_fs.rs"]
mod filesystem;

#[tokio::test]
async fn startup_and_interval_renew_from_leaf_expiry_and_reuse_the_acme_account() -> Result<()> {
    let remote = fixture::Fixture::start().await?;
    let temp = tempfile::tempdir()?;
    let profile = temp.path().join("connections.json");
    let ddns = temp.path().join("ddns.json");
    let dns = remote.connection("TENCENT_CLOUD");
    let mut request = remote.request();
    request.server_types = vec!["Nginx".into()];
    request.certificate_authority = "ZeroSSL".into();
    request.eab_kid = "fixture-eab".into();
    request.eab_hmac =
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, [7u8; 32]);
    set_expiry(&remote, 2);
    remote.state.inner.lock().unwrap().required_eab = true;
    Store {
        version: 1,
        connections: vec![dns.clone(), request.storage.clone()],
        ..Store::default()
    }
    .save(&profile)?;
    let rows = || cloud::execute_at(&dns, Action::Domains, ddns.clone());
    assert_eq!(rows().await?.rows[0]["certificateStatus"], "未申请");
    renewal::issue(
        profile.clone(),
        dns.clone(),
        request,
        Options {
            auto_renew: true,
            deployment: None,
        },
        &Control::default(),
    )
    .await?;
    let before = rows().await?;
    assert_eq!(before.rows[0]["certificateStatus"], "即将到期");
    assert_ne!(before.rows[0]["certificateExpiry"], "");
    cloud::api::Client::new(&dns)?
        .save_record(
            "owned.test",
            &Record {
                rr: "@".into(),
                kind: "A".into(),
                value: "192.0.2.10".into(),
                ttl: 600,
                line: "default".into(),
                ..Record::default()
            },
        )
        .await?;
    let records = cloud::execute_at(
        &dns,
        Action::Records {
            domain: "owned.test".into(),
        },
        ddns.clone(),
    )
    .await?;
    assert_eq!(
        records.rows[0]["certificateExpiry"],
        before.rows[0]["certificateExpiry"]
    );
    set_expiry(&remote, 30);
    remote.state.inner.lock().unwrap().deny_authorization = true;
    let runner = Runner::with_interval(
        profile.clone(),
        &tokio::runtime::Handle::current(),
        Duration::from_secs(5),
        || {},
    );
    assert!(notice(&runner).await?.contains("证书续期失败"));
    assert_eq!(
        rows().await?.rows[0]["certificateStatus"],
        "即将到期 · 续期失败"
    );
    remote.state.inner.lock().unwrap().deny_authorization = false;
    assert!(notice(&runner).await?.contains("证书已保存"));
    let after = rows().await?;
    assert_eq!(after.rows[0]["certificateStatus"], "已签发");
    assert_ne!(
        after.rows[0]["certificateExpiry"],
        before.rows[0]["certificateExpiry"]
    );
    let state = remote.state.inner.lock().unwrap();
    assert_eq!(
        state.calls.iter().filter(|c| *c == "/acme/account").count(),
        1
    );
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|c| *c == "/acme/new-order")
            .count(),
        3
    );
    assert!(!state.records.values().any(|r| r.kind == "TXT"));
    Ok(())
}

#[tokio::test]
async fn ssh_only_issuance_retries_publication_without_another_order() -> Result<()> {
    let remote = fixture::Fixture::start().await?;
    let temp = tempfile::tempdir()?;
    let profile = temp.path().join("connections.json");
    let server = tempfile::tempdir()?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let mut ssh = Connection::new("SSH");
    ssh.set("port", listener.local_addr()?.port());
    ssh.set("username", "fixture");
    ssh.set("password", "fixture-ssh-secret");
    let mut request = remote.request();
    request.bucket.clear();
    let dns = remote.connection("ALIYUN");
    Store {
        version: 1,
        connections: vec![dns.clone(), ssh.clone()],
        ..Store::default()
    }
    .save(&profile)?;
    let attempts = Arc::new(AtomicUsize::new(0));
    let server_root = server.path().to_owned();
    let fixture_attempts = attempts.clone();
    let task = tokio::spawn(async move {
        let config = Arc::new(server::Config {
            keys: vec![PrivateKey::from(Ed25519Keypair::from_seed(&[111; 32]))],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..server::Config::default()
        });
        let mut sessions = tokio::task::JoinSet::new();
        for _ in 0..2 {
            let (socket, _) = listener.accept().await?;
            let handler = SshPeer {
                root: server_root.clone(),
                attempts: fixture_attempts.clone(),
                channels: HashMap::new(),
                sftp: tokio::task::JoinSet::new(),
            };
            let config = config.clone();
            sessions.spawn(async move { server::run_stream(config, socket, handler).await?.await });
        }
        // The client closes each transport independently. The first connection
        // closing must not cancel the second, which may still be authenticating.
        while sessions.join_next().await.is_some() {}
        Ok::<(), anyhow::Error>(())
    });
    let output = renewal::issue(
        profile.clone(),
        dns.clone(),
        request,
        Options {
            auto_renew: true,
            deployment: Some(Deployment {
                ssh_id: ssh.id,
                directory: "/tls/owned.test/".into(),
                reload_command: "nginx -t && nginx -s reload".into(),
            }),
        },
        &Control::default(),
    )
    .await?;
    assert!(output.contains("部署待重试"), "{output}");
    assert_eq!(attempts.load(Ordering::Relaxed), 1, "{output}");
    let config = temp.path().join("certificate-renewals.json");
    let bytes = std::fs::read_to_string(&config)?;
    assert!(bytes.starts_with(tomato_core::legacy::MARKER));
    assert!(!bytes.contains("PRIVATE KEY") && !bytes.contains("fixture-ssh-secret"));
    let runner = Runner::with_interval(
        profile,
        &tokio::runtime::Handle::current(),
        Duration::from_secs(60),
        || {},
    );
    let deployed = notice(&runner).await?;
    assert!(
        deployed.contains("已部署到 /tls/owned.test/current"),
        "{deployed}"
    );
    let rows = cloud::execute_at(&dns, Action::Domains, temp.path().join("ddns.json")).await?;
    assert_eq!(rows.rows[0]["certificateStatus"], "已签发 · 已部署");
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
    let state = remote.state.inner.lock().unwrap();
    assert!(state.writes.is_empty());
    assert_eq!(
        state
            .calls
            .iter()
            .filter(|c| *c == "/acme/new-order")
            .count(),
        1
    );
    task.abort();
    Ok(())
}

fn set_expiry(remote: &fixture::Fixture, days: i64) {
    let date = chrono::Utc::now().date_naive() + chrono::Days::new(days as u64);
    remote.state.inner.lock().unwrap().certificate_expiry =
        Some((date.year(), date.month() as u8, date.day() as u8));
}

async fn notice(runner: &Runner) -> Result<String> {
    Ok(tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            if let Some(message) = runner.take_notice() {
                return message;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await?)
}

struct SshPeer {
    root: PathBuf,
    attempts: Arc<AtomicUsize>,
    channels: HashMap<ChannelId, Channel<server::Msg>>,
    sftp: tokio::task::JoinSet<()>,
}
impl server::Handler for SshPeer {
    type Error = anyhow::Error;
    async fn auth_password(&mut self, user: &str, password: &str) -> Result<server::Auth> {
        ensure!(user == "fixture" && password == "fixture-ssh-secret");
        Ok(server::Auth::Accept)
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<()> {
        self.channels.insert(channel.id(), channel);
        reply.accept().await;
        Ok(())
    }
    async fn subsystem_request(
        &mut self,
        id: ChannelId,
        name: &str,
        session: &mut server::Session,
    ) -> Result<()> {
        ensure!(name == "sftp");
        let channel = self.channels.remove(&id).context("SFTP session")?;
        let filesystem = filesystem::Fs::new(self.root.clone());
        session.channel_success(id)?;
        self.sftp.spawn(async move {
            russh_sftp::server::run(channel.into_stream(), filesystem).await;
        });
        Ok(())
    }
    async fn exec_request(
        &mut self,
        id: ChannelId,
        bytes: &[u8],
        session: &mut server::Session,
    ) -> Result<()> {
        self.channels.remove(&id);
        let command = std::str::from_utf8(bytes)?;
        let version = command
            .split(".tomato-certificate-")
            .nth(1)
            .context("staging version")?;
        let hash = version.get(..32).context("staging version hash")?;
        ensure!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        let stage = self
            .root
            .join("tls/owned.test")
            .join(format!(".tomato-certificate-{hash}"));
        let status = if command.starts_with("umask 077; mkdir") {
            std::fs::create_dir_all(&stage)?;
            0
        } else {
            ensure!(
                command.contains("mv -Tf --") && command.contains("nginx -t && nginx -s reload")
            );
            let leaf = std::fs::read_to_string(stage.join("fullchain.pem"))?;
            let key = std::fs::read_to_string(stage.join("privkey.pem"))?;
            assert_eq!(pem::parse(leaf)?.tag(), "CERTIFICATE");
            assert_eq!(pem::parse(key)?.tag(), "PRIVATE KEY");
            u32::from(self.attempts.fetch_add(1, Ordering::Relaxed) == 0)
        };
        session.channel_success(id)?;
        session.exit_status_request(id, status)?;
        session.eof(id)?;
        session.close(id)?;
        Ok(())
    }
}
