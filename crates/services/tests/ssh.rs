use anyhow::{Result, bail};
use russh::{
    Channel, ChannelId, Pty,
    keys::{PrivateKey, PublicKey, ssh_key::private::Ed25519Keypair},
    server,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tomato_core::Connection;
use tomato_services::terminal::{Status, ssh::RemoteTerminal};

#[derive(Debug, PartialEq)]
enum Event {
    Auth,
    Size(u32, u32),
    Closed,
}
#[derive(Clone)]
struct Handler {
    events: mpsc::UnboundedSender<Event>,
    key: Option<PublicKey>,
    reject_shell: bool,
    two_factor: bool,
}
impl server::Handler for Handler {
    type Error = russh::Error;
    async fn auth_none(&mut self, user: &str) -> Result<server::Auth, Self::Error> {
        Ok(if user == "fixture-none" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        let _ = self.events.send(Event::Auth);
        self.reject_shell = user == "reject-shell";
        Ok(
            if matches!(user, "fixture" | "reject-shell")
                && password == "fixture-only"
                && self.key.is_none()
            {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        if user == "fixture-otp" && self.key.as_ref() == Some(key) {
            self.two_factor = true;
            let mut methods = russh::MethodSet::empty();
            methods.push(russh::MethodKind::KeyboardInteractive);
            return Ok(server::Auth::Reject {
                proceed_with_methods: Some(methods),
                partial_success: true,
            });
        }
        Ok(if user == "fixture" && self.key.as_ref() == Some(key) {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _submethods: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<server::Auth, Self::Error> {
        if !self.two_factor || user != "fixture-otp" {
            return Ok(server::Auth::reject());
        }
        Ok(match response {
            Some(mut response) => {
                if response
                    .next()
                    .is_some_and(|answer| answer.as_ref() == b"123456")
                {
                    server::Auth::Accept
                } else {
                    server::Auth::reject()
                }
            }
            None => server::Auth::Partial {
                name: "fixture".into(),
                instructions: "Two-factor authentication".into(),
                prompts: vec![("One-time password:".into(), false)].into(),
            },
        })
    }
    async fn channel_open_session(
        &mut self,
        _channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        cols: u32,
        rows: u32,
        _pixel_width: u32,
        _pixel_height: u32,
        modes: &[(Pty, u32)],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        assert_eq!(term, "xterm-256color");
        assert!(
            modes.contains(&(Pty::VERASE, 8)),
            "Backspace must erase in canonical script reads, not only Readline"
        );
        let _ = self.events.send(Event::Size(cols, rows));
        session.channel_success(channel)?;
        Ok(())
    }
    async fn window_change_request(
        &mut self,
        _channel: ChannelId,
        cols: u32,
        rows: u32,
        _pixel_width: u32,
        _pixel_height: u32,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let _ = self.events.send(Event::Size(cols, rows));
        Ok(())
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if self.reject_shell {
            session.channel_failure(channel)?;
            return Ok(());
        }
        // Exercise legal startup output arriving before the shell acknowledgement.
        session.data(channel, b"\x1b[31mfixture\x1b[0m\r\n".to_vec())?;
        session.channel_success(channel)?;
        Ok(())
    }
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        session.data(channel, data.to_vec())?;
        Ok(())
    }
}
struct Fixture {
    port: u16,
    task: tokio::task::JoinHandle<()>,
    events: mpsc::UnboundedReceiver<Event>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(key: Option<PublicKey>) -> Result<Self> {
        let host = PrivateKey::from(Ed25519Keypair::from_seed(&[42; 32]));
        let config = Arc::new(server::Config {
            keys: vec![host],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let (send, events) = mpsc::unbounded_channel();
        let task = tokio::spawn(async move {
            let mut jobs = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted=listener.accept()=>{
                        let Ok((socket,_))=accepted else {break};
                        let handler=Handler {events:send.clone(),key:key.clone(),reject_shell:false,two_factor:false};
                        let config=config.clone();
                        let closed=send.clone();
                        jobs.spawn(async move {
                            if let Ok(session)=server::run_stream(config,socket,handler).await {let _=session.await;}
                            let _=closed.send(Event::Closed);
                        });
                    },
                    _=jobs.join_next(),if !jobs.is_empty()=>{}
                }
            }
        });
        Ok(Self { port, task, events })
    }
    fn connection(&self) -> Connection {
        let mut c = Connection::new("SSH");
        c.set("port", u64::from(self.port));
        c.set("username", "fixture");
        c.set("password", "fixture-only");
        c
    }
    async fn event(&mut self, expected: Event) -> Result<()> {
        tokio::time::timeout(Duration::from_secs(10), async {
            while let Some(event) = self.events.recv().await {
                if event == expected {
                    return Ok(());
                }
            }
            bail!("server event channel closed")
        })
        .await?
    }
}
async fn ready(terminal: &RemoteTerminal) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            match terminal.status() {
                Status::Ready => return Ok(()),
                Status::Failed(error) => bail!("{error}"),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
    })
    .await?
}
async fn output(terminal: &mut RemoteTerminal, expected: &[u8]) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut bytes = Vec::new();
        loop {
            while let Some(data) = terminal.try_read() {
                bytes.extend(data);
            }
            if bytes.windows(expected.len()).any(|part| part == expected) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?
}
#[tokio::test]
async fn saved_password_native_shell_utf8_resize_and_drop_close_socket() -> Result<()> {
    let mut fixture = Fixture::new(None).await?;
    let dir = tempfile::tempdir()?;
    let mut terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        fixture.connection(),
        dir.path().join("known_hosts"),
    )?;
    ready(&terminal).await?;
    output(&mut terminal, b"\x1b[31mfixture").await?;
    terminal.write("中文\r")?;
    output(&mut terminal, "中文\r".as_bytes()).await?;
    terminal.resize(40, 100)?;
    fixture.event(Event::Size(100, 40)).await?;
    drop(terminal);
    fixture.event(Event::Closed).await?;
    assert!(dir.path().join("known_hosts").exists());
    Ok(())
}
#[tokio::test]
async fn missing_password_prompts_and_wrong_password_fails() -> Result<()> {
    let fixture = Fixture::new(None).await?;
    let dir = tempfile::tempdir()?;
    let mut c = fixture.connection();
    c.set("password", "");
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        c,
        dir.path().join("known_hosts"),
    )?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !matches!(terminal.status(), Status::Authentication { .. }) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    terminal.answer(vec!["fixture-only".into()])?;
    ready(&terminal).await?;
    drop(terminal);
    let mut c = fixture.connection();
    c.set("password", "wrong");
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        c,
        dir.path().join("known_hosts"),
    )?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Status::Failed(message) = terminal.status() {
                assert!(message.contains("认证失败"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    Ok(())
}
#[tokio::test]
async fn legacy_private_key_file_authenticates_without_password() -> Result<()> {
    let key = PrivateKey::from(Ed25519Keypair::from_seed(&[77; 32]));
    let fixture = Fixture::new(Some(key.public_key().clone())).await?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("fixture-key");
    std::fs::write(&path, key.to_openssh(Default::default())?.as_bytes())?;
    let mut c = fixture.connection();
    c.set("password", "");
    c.set("useKey", true);
    c.set("privateKeyPaths", serde_json::json!([path]));
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        c,
        dir.path().join("known_hosts"),
    )?;
    ready(&terminal).await?;
    Ok(())
}
#[tokio::test]
async fn changed_host_key_is_rejected_before_sending_password() -> Result<()> {
    let mut fixture = Fixture::new(None).await?;
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("known_hosts");
    let other = PrivateKey::from(Ed25519Keypair::from_seed(&[99; 32]));
    russh::keys::known_hosts::learn_known_hosts_path(
        "127.0.0.1",
        fixture.port,
        other.public_key(),
        &path,
    )?;
    let original = std::fs::read(&path)?;
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        fixture.connection(),
        path.clone(),
    )?;
    assert!(ready(&terminal).await.is_err());
    assert_eq!(std::fs::read(&path)?, original);
    while let Ok(event) = fixture.events.try_recv() {
        assert_ne!(event, Event::Auth);
    }
    Ok(())
}

#[tokio::test]
async fn encrypted_key_prompts_and_a_missing_earlier_key_does_not_hide_it() -> Result<()> {
    let key = PrivateKey::from(Ed25519Keypair::from_seed(&[77; 32]));
    let fixture = Fixture::new(Some(key.public_key().clone())).await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("encrypted-key");
    let encrypted = key.encrypt(&mut rand::rng(), "fixture-passphrase")?;
    std::fs::write(&path, encrypted.to_openssh(Default::default())?.as_bytes())?;
    let mut connection = fixture.connection();
    connection.set("useKey", true);
    connection.set("usePassword", false);
    connection.set("password", "");
    connection.set(
        "privateKeyPaths",
        serde_json::json!([directory.path().join("missing-key"), path]),
    );
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        connection,
        directory.path().join("known_hosts"),
    )?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !matches!(terminal.status(), Status::Authentication { .. }) {
            if let Status::Failed(error) = terminal.status() {
                bail!("{error}");
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    terminal.answer(vec!["fixture-passphrase".into()])?;
    ready(&terminal).await?;
    Ok(())
}

#[tokio::test]
async fn a_rejected_shell_never_becomes_ready() -> Result<()> {
    let fixture = Fixture::new(None).await?;
    let mut connection = fixture.connection();
    connection.set("username", "reject-shell");
    let directory = tempfile::tempdir()?;
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        connection,
        directory.path().join("known_hosts"),
    )?;
    let error = ready(&terminal).await.unwrap_err().to_string();
    assert!(error.contains("Shell") && error.contains("拒绝"), "{error}");
    Ok(())
}

#[tokio::test]
async fn public_key_plus_otp_works_when_password_authentication_is_disabled() -> Result<()> {
    let key = PrivateKey::from(Ed25519Keypair::from_seed(&[77; 32]));
    let fixture = Fixture::new(Some(key.public_key().clone())).await?;
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fixture-key");
    std::fs::write(&path, key.to_openssh(Default::default())?.as_bytes())?;
    let mut connection = fixture.connection();
    connection.set("username", "fixture-otp");
    connection.set("useKey", true);
    connection.set("usePassword", false);
    connection.set("privateKeyPaths", serde_json::json!([path]));
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        connection,
        directory.path().join("known_hosts"),
    )?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match terminal.status() {
                Status::Authentication { prompts, .. } => {
                    assert_eq!(prompts[0].label, "One-time password:");
                    assert!(!prompts[0].echo);
                    break;
                }
                Status::Failed(error) => bail!("{error}"),
                _ => tokio::time::sleep(Duration::from_millis(5)).await,
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    terminal.answer(vec!["123456".into()])?;
    ready(&terminal).await?;
    Ok(())
}

#[tokio::test]
async fn a_server_accepting_none_auth_does_not_prompt_for_a_password() -> Result<()> {
    let fixture = Fixture::new(None).await?;
    let mut connection = fixture.connection();
    connection.set("username", "fixture-none");
    connection.set("usePassword", false);
    connection.set("password", "");
    let directory = tempfile::tempdir()?;
    let terminal = RemoteTerminal::start(
        &tokio::runtime::Handle::current(),
        connection,
        directory.path().join("known_hosts"),
    )?;
    ready(&terminal).await?;
    Ok(())
}
