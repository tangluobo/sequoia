//! SMB shares use the same saved server configuration as HTTP and FTP.
use crate::network::RunningServer;
use anyhow::{Context, Result, ensure};
use smb_server::{Access, LocalFsBackend, Share, SmbServer};
use std::{collections::HashSet, net::SocketAddr, time::Duration};
use tokio_util::sync::CancellationToken;
use tomato_core::server::{ServerAccount, ServerConfig, SharedDirectory};

pub(super) async fn start(config: ServerConfig) -> Result<RunningServer> {
    config.validate()?;
    let address = tokio::net::lookup_host((config.bind_address.as_str(), config.port))
        .await
        .context("无法解析 SMB 绑定地址")?
        .next()
        .context("SMB 绑定地址没有可用的 IP")?;
    let server = tokio::task::spawn_blocking(move || prepare(config, address)).await??;
    let address = server
        .bind()
        .await
        .with_context(|| format!("无法监听 SMB 地址 {address}"))?;
    let shutdown = CancellationToken::new();
    let stopped = shutdown.clone();
    let handle = server.shutdown_handle();
    let task = tokio::spawn(async move {
        let service = server.serve();
        tokio::pin!(service);
        tokio::select! {
            _ = &mut service => {}
            _ = stopped.cancelled() => {
                handle.shutdown();
                let _ = tokio::time::timeout(Duration::from_secs(3), service).await;
            }
        }
    });
    Ok(RunningServer::from_task(address, task).with_shutdown(shutdown))
}

fn prepare(config: ServerConfig, address: SocketAddr) -> Result<SmbServer> {
    let accounts: Vec<_> = config
        .accounts
        .iter()
        .filter(|account| account.enabled)
        .collect();
    let mut names = HashSet::new();
    let public = config.anonymous_access && config.shared_directories.iter().any(public_read_only);
    let mut builder = SmbServer::builder()
        .listen(address)
        .require_signing(!public)
        .max_connections(64)
        .max_sessions_per_connection(8)
        .max_opens_per_connection(256)
        .max_read_size(1024 * 1024)
        .max_write_size(1024 * 1024);
    for account in &accounts {
        ensure!(
            names.insert(account.username.to_ascii_lowercase()),
            "SMB 账号名称重复：{}",
            account.username
        );
        builder = builder.user(account.username.as_str(), account.password.as_str());
    }

    let mut shares = config.shared_directories;
    if shares.is_empty() && !config.root_directory.is_empty() {
        shares.push(SharedDirectory {
            alias: "root".into(),
            path: config.root_directory,
            ..Default::default()
        });
    }
    for account in accounts
        .iter()
        .filter(|account| !account.home_directory.is_empty())
    {
        shares.push(SharedDirectory {
            alias: account.username.clone(),
            path: account.home_directory.clone(),
            allowed_users: vec![account.username.clone()],
            ..Default::default()
        });
    }
    ensure!(
        !shares.is_empty(),
        "请为 SMB 配置共享目录、根目录或账号主目录"
    );
    for directory in &shares {
        builder = builder.share(share(directory, &accounts, config.anonymous_access)?);
    }
    builder.build().context("SMB 服务器配置无效")
}

fn public_read_only(directory: &SharedDirectory) -> bool {
    directory.read_only && directory.allowed_users.is_empty()
}

fn share(
    directory: &SharedDirectory,
    accounts: &[&ServerAccount],
    anonymous: bool,
) -> Result<Share> {
    let backend = LocalFsBackend::new(&directory.path)
        .with_context(|| format!("无法打开 SMB 共享目录 {}", directory.alias))?;
    // Shared files can also be edited locally or through a second share.
    let mut share = Share::new(directory.alias.as_str(), backend)
        .leases(false)
        .durable_handles(false);
    if anonymous && public_read_only(directory) {
        return Ok(share.public_read_only());
    }
    let access = if directory.read_only {
        Access::Read
    } else {
        Access::ReadWrite
    };
    for account in accounts {
        if directory.allowed_users.is_empty()
            || directory
                .allowed_users
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&account.username))
        {
            share = share.user(account.username.as_str(), access);
        }
    }
    Ok(share)
}
