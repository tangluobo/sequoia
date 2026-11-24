use anyhow::{Context, Result};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tomato_core::server::{ServerAccount, ServerConfig, ServerType, SharedDirectory};

async fn negotiate(socket: &mut TcpStream) -> Result<Vec<u8>> {
    let mut message = [0u8; 102];
    message[..4].copy_from_slice(b"\xFESMB");
    message[4..6].copy_from_slice(&64u16.to_le_bytes());
    message[14..16].copy_from_slice(&1u16.to_le_bytes());
    message[64..66].copy_from_slice(&36u16.to_le_bytes());
    message[66..68].copy_from_slice(&1u16.to_le_bytes());
    message[68..70].copy_from_slice(&1u16.to_le_bytes());
    message[76..92].copy_from_slice(&[42; 16]);
    message[100..102].copy_from_slice(&0x0210u16.to_le_bytes());
    socket
        .write_all(&(message.len() as u32).to_be_bytes())
        .await?;
    socket.write_all(&message).await?;
    let len = socket.read_u32().await? as usize;
    anyhow::ensure!(
        (70..65536).contains(&len),
        "invalid SMB response length {len}"
    );
    let mut response = vec![0; len];
    socket.read_exact(&mut response).await?;
    Ok(response)
}

#[tokio::test]
async fn smb_dispatcher_binds_real_signed_smb2_and_releases_clients_on_stop() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        let root = tempfile::tempdir()?;
        let config = ServerConfig {
            kind: ServerType::Smb,
            bind_address: "127.0.0.1".into(),
            port: 0,
            accounts: vec![ServerAccount {
                username: "fixture".into(),
                password: "fixture-only".into(),
                ..Default::default()
            }],
            shared_directories: vec![SharedDirectory {
                alias: "files".into(),
                path: root.path().to_string_lossy().into_owned(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let server = tomato_services::servers::start(config.clone()).await?;
        let address = server.address;
        assert!(address.ip().is_loopback());
        let mut collision = config;
        collision.port = address.port();
        let error = tomato_services::servers::start(collision)
            .await
            .err()
            .context("must report an occupied port")?;
        assert!(format!("{error:#}").contains("无法监听 SMB 地址"));
        let mut socket = TcpStream::connect(address).await?;
        let response = negotiate(&mut socket).await?;
        assert_eq!(&response[..4], b"\xFESMB");
        assert_eq!(&response[8..12], &[0; 4]);
        assert_eq!(&response[64..66], &65u16.to_le_bytes());
        assert_eq!(response[66] & 2, 2, "SMB signing must be required");
        assert_eq!(&response[68..70], &0x0210u16.to_le_bytes());

        drop(server);
        assert!(matches!(socket.read(&mut [0]).await, Ok(0) | Err(_)));
        tokio::time::timeout(Duration::from_secs(3), async {
            while TcpListener::bind(address).await.is_err() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .context("SMB listener survived shutdown")?;
        Ok(())
    })
    .await?
}

#[tokio::test]
#[ignore = "serves a dedicated loopback share for an independent SMB client"]
async fn host_owned_smb_client_fixture() -> Result<()> {
    use std::path::{Path, PathBuf};
    let directory = PathBuf::from(
        std::env::var_os("TOMATO_OWNED_SMB_TEST_ROOT").context("missing SMB fixture directory")?,
    );
    anyhow::ensure!(
        directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("tomato-smb-fixture-")),
        "requires an owned SMB fixture directory"
    );
    let directory = directory.canonicalize()?;
    let root = |name: &str| -> Result<String> {
        let path = directory.join(name);
        std::fs::create_dir(&path)?;
        Ok(path.to_string_lossy().into_owned())
    };
    let share = |alias: &str, read_only: bool, users: &[&str]| -> Result<SharedDirectory> {
        Ok(SharedDirectory {
            alias: alias.into(),
            path: root(alias)?,
            read_only,
            allowed_users: users.iter().copied().map(str::to_owned).collect(),
            ..Default::default()
        })
    };
    let public = share("read", true, &[])?;
    let restricted = share("restricted", true, &["reader"])?;
    std::fs::write(
        Path::new(&public.path).join("只读.txt"),
        b"public read only",
    )?;
    std::fs::write(Path::new(&restricted.path).join("私有.txt"), b"restricted")?;
    let account = |name: &str, enabled| ServerAccount {
        username: name.into(),
        password: "smb-fixture-only".into(),
        enabled,
        ..Default::default()
    };
    let mut writer = account("writer", true);
    writer.home_directory = root("home")?;
    std::fs::write(Path::new(&writer.home_directory).join("个人.txt"), b"home")?;
    let server = tomato_services::servers::start(ServerConfig {
        kind: ServerType::Smb,
        bind_address: "127.0.0.1".into(),
        port: 0,
        anonymous_access: true,
        accounts: vec![writer, account("reader", true), account("disabled", false)],
        shared_directories: vec![share("work", false, &["writer"])?, public, restricted],
        ..Default::default()
    })
    .await?;
    tokio::fs::write(directory.join("ready"), server.address.to_string()).await?;
    tokio::time::timeout(Duration::from_secs(120), async {
        while !directory.join("stop").exists() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    drop(server);
    Ok(())
}
