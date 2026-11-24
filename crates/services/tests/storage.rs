//! Native adapters are exercised with disposable files and loopback servers only.
use anyhow::{Result, ensure};
use std::{path::Path, time::Duration};
use tomato_core::Connection;
#[path = "storage/documents.rs"]
mod documents;
#[path = "storage/upload.rs"]
mod upload;
use tomato_services::{
    files::{
        operations::{Operation, Output},
        session::FileSession,
        storage::Storage,
    },
    terminal::Status,
};
fn connection(root: &Path) -> Connection {
    let mut c = Connection::new("LOCAL_DIRECTORY");
    c.set("localDirectoryPath", root.to_string_lossy().to_string());
    c
}
fn remote(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
#[tokio::test]
async fn local_adapter_handles_crud_recursive_transfers_and_root_guards() -> Result<()> {
    let root = tempfile::tempdir()?;
    let files = tempfile::tempdir()?;
    let destination = tempfile::tempdir()?;
    let storage = Storage::new(&connection(root.path()))?;
    storage.check().await?;
    let directory = root.path().join("原目录");
    let path = remote(&directory);
    storage.execute(Operation::Mkdir(path.clone())).await?;
    let text = format!("{path}/配置.json");
    storage.execute(Operation::Create(text.clone())).await?;
    assert!(
        storage
            .execute(Operation::Create(text.clone()))
            .await
            .is_err()
    );
    storage
        .execute(Operation::WriteText {
            path: text.clone(),
            text: "{\"中文\":true}".into(),
        })
        .await?;
    let Output::Text { text: value, .. } =
        storage.execute(Operation::ReadText(text.clone())).await?
    else {
        anyhow::bail!("Not text")
    };
    assert_eq!(value, "{\"中文\":true}");
    let content: Vec<_> = (0..270_000).map(|i| (i % 256) as u8).collect();
    tokio::fs::create_dir(files.path().join("sub")).await?;
    tokio::fs::write(files.path().join("sub/中文.bin"), &content).await?;
    storage
        .execute(Operation::Upload {
            local: files.path().into(),
            remote: format!("{path}/上传"),
        })
        .await?;
    storage
        .execute(Operation::Download {
            remote: path.clone(),
            local: destination.path().join("copy"),
        })
        .await?;
    assert_eq!(
        tokio::fs::read(destination.path().join("copy/上传/sub/中文.bin")).await?,
        content
    );
    let renamed = remote(&root.path().join("新目录"));
    storage
        .execute(Operation::Rename {
            from: path,
            to: renamed.clone(),
        })
        .await?;
    let Output::Directory { entries, .. } = storage.list(&remote(root.path())).await? else {
        anyhow::bail!("Not a directory")
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "新目录");
    assert!(entries[0].directory);
    assert!(
        storage
            .execute(Operation::Rename {
                from: renamed.clone(),
                to: format!("{renamed}/child")
            })
            .await
            .is_err()
    );
    storage
        .execute(Operation::Remove {
            path: renamed,
            directory: true,
        })
        .await?;
    assert!(!root.path().join("新目录").exists());
    assert!(
        storage
            .execute(Operation::Remove {
                path: "/".into(),
                directory: true
            })
            .await
            .is_err()
    );
    assert!(storage.list("/../../").await.is_err());
    Ok(())
}
async fn request(session: &mut FileSession, operation: Operation) -> Result<Output> {
    let id = session.request(operation)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(reply) = session.try_reply()
                && reply.id == id
            {
                return reply.result.map_err(anyhow::Error::msg);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?
}
#[tokio::test]
async fn file_actor_reuploads_external_edits_and_drops_its_cache() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("编辑.md");
    tokio::fs::write(&path, "before").await?;
    let mut session =
        FileSession::start(&tokio::runtime::Handle::current(), connection(root.path()));
    tokio::time::timeout(Duration::from_secs(5), async {
        while session.status() == Status::Connecting {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await?;
    ensure!(
        session.status() == Status::Ready,
        "{session_status:?}",
        session_status = session.status()
    );
    let Output::EditCached { local, .. } = request(
        &mut session,
        Operation::PrepareEdit {
            remote: remote(&path),
            launch: false,
        },
    )
    .await?
    else {
        anyhow::bail!("No editor cache")
    };
    tokio::fs::write(&local, "after 中文").await?;
    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if tokio::fs::read_to_string(&path).await? == "after 中文" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok::<_, std::io::Error>(())
    })
    .await??;
    drop(session);
    tokio::time::timeout(Duration::from_secs(3), async {
        while local.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn native_ftp_client_and_server_transfer_files_and_enforce_account_permissions() -> Result<()>
{
    use tomato_core::server::{ServerAccount, ServerConfig, ServerType, SharedDirectory};
    let root = tempfile::tempdir()?;
    let readonly = tempfile::tempdir()?;
    let files = tempfile::tempdir()?;
    tokio::fs::write(readonly.path().join("只读.txt"), "read only").await?;
    let long_text = "逐块读取中文文件🍅\n".repeat(10_000);
    tokio::fs::write(readonly.path().join("长文本.txt"), &long_text).await?;
    let mut config = ServerConfig {
        kind: ServerType::Ftp,
        port: 0,
        bind_address: "127.0.0.1".into(),
        root_directory: remote(root.path()),
        accounts: vec![
            ServerAccount {
                username: "fixture".into(),
                password: "fixture-only".into(),
                ..Default::default()
            },
            ServerAccount {
                username: "reader".into(),
                password: "read-only".into(),
                home_directory: remote(readonly.path()),
                ..Default::default()
            },
        ],
        shared_directories: vec![SharedDirectory {
            alias: "readonly".into(),
            path: remote(readonly.path()),
            read_only: true,
            allowed_users: vec!["reader".into()],
            ..Default::default()
        }],
        ..Default::default()
    };
    let server = tomato_services::servers::ftp::start(config.clone()).await?;
    let mut c = Connection::new("FTP");
    c.set("host", "127.0.0.1");
    c.set("port", server.address.port());
    c.set("username", "fixture");
    c.set("password", "fixture-only");
    let storage = Storage::new(&c)?;
    storage.check().await?;
    storage.execute(Operation::Mkdir("/上传".into())).await?;
    let bytes: Vec<_> = (0..270_000).map(|n| (n % 256) as u8).collect();
    let local = files.path().join("源.bin");
    tokio::fs::write(&local, &bytes).await?;
    storage
        .execute(Operation::Upload {
            local,
            remote: "/上传/中文.bin".into(),
        })
        .await?;
    let Output::Directory { entries, .. } = storage.list("/上传").await? else {
        anyhow::bail!("No FTP listing")
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "中文.bin");
    assert_eq!(entries[0].size, bytes.len() as u64);
    storage
        .execute(Operation::Rename {
            from: "/上传/中文.bin".into(),
            to: "/上传/renamed.bin".into(),
        })
        .await?;
    let download = files.path().join("下载.bin");
    storage
        .execute(Operation::Download {
            remote: "/上传/renamed.bin".into(),
            local: download.clone(),
        })
        .await?;
    assert_eq!(tokio::fs::read(download).await?, bytes);
    let Output::ExportReady(_, export) = storage
        .execute(Operation::ExportFile(
            tomato_services::files::export::Request::new("/上传/renamed.bin".into()),
        ))
        .await?
    else {
        anyhow::bail!("FTP drag export missing")
    };
    assert_eq!(tokio::fs::read(export.path()).await?, bytes);
    let selection = tomato_services::files::export::Request::selection(vec![
        tomato_services::files::export::Item::new(
            "/上传/renamed.bin".into(),
            Some(bytes.len() as u64),
            false,
        ),
        tomato_services::files::export::Item::new("/上传".into(), None, true),
    ])?;
    let copying = storage.execute(Operation::ExportFile(selection.clone()));
    deferred_export::assert_after_drop(selection, copying, &bytes, "/上传/renamed.bin").await?;

    assert_eq!(
        tokio::fs::read(root.path().join("上传/renamed.bin")).await?,
        bytes
    );
    c.set("username", "reader");
    c.set("password", "read-only");
    let reader = Storage::new(&c)?;
    reader.check().await?;
    let Output::Text { text, .. } = reader
        .execute(Operation::ReadText("/只读.txt".into()))
        .await?
    else {
        anyhow::bail!("No FTP text")
    };
    assert_eq!(text, "read only");
    let Output::Text { text, .. } = reader
        .execute(Operation::ReadText("/长文本.txt".into()))
        .await?
    else {
        anyhow::bail!("No long FTP text")
    };
    assert_eq!(text, long_text);
    assert!(
        reader
            .execute(Operation::WriteText {
                path: "/拒绝.txt".into(),
                text: "must not write".into()
            })
            .await
            .is_err()
    );
    assert!(
        reader
            .execute(Operation::Remove {
                path: "/只读.txt".into(),
                directory: false
            })
            .await
            .is_err()
    );
    assert!(!readonly.path().join("拒绝.txt").exists());
    assert_eq!(
        tokio::fs::read_to_string(readonly.path().join("只读.txt")).await?,
        "read only"
    );
    c.set("password", "wrong");
    assert!(Storage::new(&c)?.check().await.is_err());
    storage
        .execute(Operation::Remove {
            path: "/上传".into(),
            directory: true,
        })
        .await?;
    config.accounts[0].enabled = false;
    server.update_config(config)?;
    tokio::time::timeout(Duration::from_secs(3), async {
        while storage.list("/").await.is_ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    drop(server);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        tokio::net::TcpStream::connect(("127.0.0.1", c.number("port") as u16))
            .await
            .is_err()
    );
    Ok(())
}
#[path = "support/deferred_export.rs"]
mod deferred_export;
