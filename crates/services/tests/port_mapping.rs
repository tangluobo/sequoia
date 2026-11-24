//! Real SSH L/R channels, loopback listeners, independent legacy configuration.
use anyhow::{Result, ensure};
use russh::{
    Channel,
    keys::{PrivateKey, ssh_key::private::Ed25519Keypair},
    server,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tomato_services::terminal::ssh::forwarding::{Forward, Mapping, Repository};
#[path = "port_mapping/recovery.rs"]
mod recovery;
#[derive(Default)]
struct Handler {
    tasks: tokio::task::JoinSet<()>,
}
impl server::Handler for Handler {
    type Error = anyhow::Error;
    async fn auth_password(&mut self, user: &str, password: &str) -> Result<server::Auth> {
        Ok(if user == "fixture" && password == "fixture-only" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<()> {
        ensure!(host == "127.0.0.1", "Loopback only");
        let mut socket = TcpStream::connect((host, u16::try_from(port)?)).await?;
        reply.accept().await;
        self.tasks.spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
        });
        Ok(())
    }
    async fn tcpip_forward(
        &mut self,
        host: &str,
        port: &mut u32,
        session: &mut server::Session,
    ) -> Result<bool> {
        ensure!(host == "127.0.0.1", "Loopback only");
        let listener = TcpListener::bind((host, u16::try_from(*port)?)).await?;
        *port = u32::from(listener.local_addr()?.port());
        let port = *port;
        let handle = session.handle();
        self.tasks.spawn(async move{let mut jobs=tokio::task::JoinSet::new();loop{tokio::select!{
            accepted=listener.accept()=>{let Ok((mut socket,origin))=accepted else{break;};let handle=handle.clone();jobs.spawn(async move{if let Ok(channel)=handle.channel_open_forwarded_tcpip("127.0.0.1",port,origin.ip().to_string(),u32::from(origin.port())).await{let mut stream=channel.into_stream();let _=tokio::io::copy_bidirectional(&mut socket,&mut stream).await;}});},
            _=jobs.join_next(),if !jobs.is_empty()=>{},
        }}});
        Ok(true)
    }
}
struct Task(tokio::task::JoinHandle<()>);
impl Drop for Task {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0").await?.local_addr()?.port())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ssh_mapping_both_directions_transfer_and_release_ports() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(30),async{
        let directory=tempfile::tempdir()?;
        let listener=TcpListener::bind("127.0.0.1:0").await?;let port=listener.local_addr()?.port();
        let config=Arc::new(server::Config{keys:vec![PrivateKey::from(Ed25519Keypair::from_seed(&[121;32]))],auth_rejection_time:Duration::ZERO,auth_rejection_time_initial:Some(Duration::ZERO),..Default::default()});
        let _ssh=Task(tokio::spawn(async move{let mut jobs=tokio::task::JoinSet::new();loop{tokio::select!{socket=listener.accept()=>{let Ok((socket,_))=socket else{break;};let config=config.clone();jobs.spawn(async move{if let Ok(session)=server::run_stream(config,socket,Handler::default()).await{let _=session.await;}});},_=jobs.join_next(),if !jobs.is_empty()=>{}}}}));
        let echo=TcpListener::bind("127.0.0.1:0").await?;let echo_port=echo.local_addr()?.port();
        let _echo=Task(tokio::spawn(async move{let mut jobs=tokio::task::JoinSet::new();loop{tokio::select!{socket=echo.accept()=>{let Ok((mut socket,_))=socket else{break;};jobs.spawn(async move{let mut buffer=[0;8192];while let Ok(n)=socket.read(&mut buffer).await{if n==0{break;}
 if socket.write_all(&buffer[..n]).await.is_err(){break;}}});},_=jobs.join_next(),if !jobs.is_empty()=>{}}}}));
        let mut host=tomato_core::Connection::new("SSH");host.set("host","127.0.0.1");host.set("port",port);host.set("username","fixture");host.set("password","fixture-only");host.set("usePassword",true);
        for direction in ["L","R"]{
            let forwarded=free_port().await?;
            let mapping=Mapping{ssh_host_id:host.id.clone(),direction:direction.into(),local_port:if direction=="L"{forwarded}else{echo_port},remote_port:if direction=="L"{echo_port}else{forwarded},..Default::default()};
            let forward=Forward::open(mapping,host.clone(),directory.path().join("known_hosts")).await?;
            ensure!(forward.is_running(),"Tunnel closed prematurely");
            let mut stream=TcpStream::connect(("127.0.0.1",forwarded)).await?;
            let bytes:Vec<u8>=(0..128*1024).map(|i|(i%251) as u8).collect();
            let (mut reader,mut writer)=stream.split();let output=bytes.clone();
            let send=async move{writer.write_all(&output).await?;writer.shutdown().await};
            let receive=async move{let mut actual=vec![0;bytes.len()];reader.read_exact(&mut actual).await?;ensure!(actual==bytes,"SSH tunnel payload mismatch");Ok::<_,anyhow::Error>(())};
            let (sent,received)=tokio::join!(send,receive);sent?;received?;drop(stream);drop(forward);
            for _ in 0..100{if TcpListener::bind(("127.0.0.1",forwarded)).await.is_ok(){break;}tokio::time::sleep(Duration::from_millis(10)).await;}
            let _released=TcpListener::bind(("127.0.0.1",forwarded)).await?;
        }
        Ok::<_,anyhow::Error>(())
    }).await?
}
#[test]
fn legacy_mapping_unknown_fields_backup_and_external_change_guard() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("port-mappings.json");
    let original=br#"[{"id":"old-id","name":"legacy","sshHostId":"ssh-id","direction":"R","localHost":"127.0.0.1","localPort":1234,"remoteHost":"0.0.0.0","remotePort":5678,"futureOption":{"enabled":true}}]"#;
    std::fs::write(&path, original)?;
    let (mut repository, mut mappings) = Repository::load(path.clone())?;
    assert_eq!(mappings[0].id, "old-id");
    mappings[0].name = "updated".into();
    repository.save(&mappings)?;
    assert_eq!(
        std::fs::read(path.with_extension("json.rust-backup"))?,
        original
    );
    let actual: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    assert_eq!(actual[0]["futureOption"]["enabled"], true);
    std::fs::write(&path, b"[]")?;
    assert!(repository.save(&mappings).is_err());
    assert_eq!(std::fs::read(&path)?, b"[]");
    Ok(())
}

#[test]
fn legacy_nullable_mapping_defaults_and_duplicate_id_rejection() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("port-mappings.json");
    std::fs::write(&path,br#"[{"id":null,"name":null,"sshHostId":null,"direction":null,"localHost":null,"remoteHost":null}]"#)?;
    let (_, mappings) = Repository::load(path.clone())?;
    assert!(!mappings[0].id.is_empty());
    assert_eq!(mappings[0].direction, "L");
    assert_eq!(mappings[0].local_host, "127.0.0.1");
    std::fs::write(&path, b"null")?;
    assert!(Repository::load(path.clone())?.1.is_empty());
    std::fs::write(&path, br#"[{"id":"duplicate"},{"id":"duplicate"}]"#)?;
    assert!(Repository::load(path).is_err());
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "owned GUI integration fixture; requires TOMATO_PORT_FIXTURE_DIR"]
async fn host_port_mapping_fixture() -> Result<()> {
    let root = std::path::PathBuf::from(
        std::env::var_os("TOMATO_PORT_FIXTURE_DIR")
            .ok_or_else(|| anyhow::anyhow!("Missing isolated fixture directory"))?,
    );
    ensure!(
        root.file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("tomato-port-fixture-")),
        "Requires owned fixture directory"
    );
    std::fs::create_dir_all(&root)?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let ssh_port = listener.local_addr()?.port();
    let config = Arc::new(server::Config {
        keys: vec![PrivateKey::from(Ed25519Keypair::from_seed(&[121; 32]))],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let echo = TcpListener::bind("127.0.0.1:0").await?;
    let echo_port = echo.local_addr()?.port();
    let mut host = tomato_core::Connection::new("SSH");
    host.id = "owned-ssh".into();
    host.name = "owned-loopback-ssh".into();
    host.set("host", "127.0.0.1");
    host.set("port", ssh_port);
    host.set("username", "fixture");
    host.set("password", "fixture-only");
    host.set("usePassword", true);
    std::fs::write(
        root.join("ready.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"connection":host,"echoPort":echo_port}))?,
    )?;
    let mut sessions = tokio::task::JoinSet::new();
    let mut poll = tokio::time::interval(Duration::from_millis(100));
    let started = std::time::Instant::now();
    loop {
        tokio::select! {
                socket=listener.accept()=>{let(socket,_)=socket?;let config=config.clone();sessions.spawn(async move{if let Ok(session)=server::run_stream(config,socket,Handler::default()).await{let _=session.await;}});},
                socket=echo.accept()=>{let(mut socket,_)=socket?;sessions.spawn(async move{let mut bytes=[0;8192];while let Ok(count)=socket.read(&mut bytes).await{if count==0{break;}
        if socket.write_all(&bytes[..count]).await.is_err(){break;}}});},
                _=sessions.join_next(),if !sessions.is_empty()=>{},
                _=poll.tick()=>{if root.join("stop").exists(){break;}ensure!(started.elapsed()<Duration::from_secs(180),"Owned fixture deadline elapsed");},
            }
    }
    Ok(())
}
