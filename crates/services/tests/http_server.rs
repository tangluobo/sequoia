use anyhow::{Context as _, Result, ensure};
use reqwest::{Client, StatusCode};
use std::time::Duration;
use tomato_core::server::{ServerAccount, ServerConfig, SharedDirectory};

fn account(name: &str) -> ServerAccount {
    ServerAccount {
        username: name.into(),
        password: "fixture-only".into(),
        ..Default::default()
    }
}
fn link(html: &str, label: &str) -> Result<String> {
    let before = html.split(label).next().unwrap_or_default();
    let href = before
        .rsplit_once("href=\"")
        .map(|(_, value)| value.split('"').next().unwrap_or_default());
    href.map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("Missing link for {label}"))
}
#[tokio::test]
async fn http_shared_accounts_tokens_crud_and_live_policy() -> Result<()> {
    tomato_services::init_crypto();
    let public = tempfile::tempdir()?;
    let private = tempfile::tempdir()?;
    tokio::fs::write(public.path().join("中文 空格+.txt"), "原版下载链接").await?;
    tokio::fs::write(private.path().join("secret.txt"), "private").await?;
    let mut config = ServerConfig {
        bind_address: "127.0.0.1".into(),
        port: 0,
        accounts: vec![account("owner"), account("other")],
        shared_directories: vec![
            SharedDirectory {
                alias: "公共 文件".into(),
                path: public.path().to_string_lossy().into(),
                ..Default::default()
            },
            SharedDirectory {
                alias: "restricted".into(),
                path: private.path().to_string_lossy().into(),
                read_only: true,
                allowed_users: vec!["owner".into()],
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let server = tomato_services::servers::http::start(config.clone()).await?;
    let base = format!("http://{}", server.address);
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;
    let unauthenticated = client.get(&base).send().await?;
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        unauthenticated.headers()["www-authenticate"],
        "Basic realm=\"File Server\""
    );
    let html = client
        .get(&base)
        .basic_auth("other", Some("fixture-only"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(!html.contains("restricted"));
    let public_link = link(&html, "📂 公共 文件")?;
    let directory = client
        .get(format!("{base}{public_link}"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    assert!(directory.contains("padding:20px"));
    let download = link(&directory, "📄 中文 空格+.txt")?;
    let downloaded = client
        .get(format!("{base}{download}"))
        .send()
        .await?
        .error_for_status()?;
    assert!(
        downloaded.headers()["content-disposition"]
            .to_str()?
            .contains("%E4%B8%AD")
    );
    assert_eq!(downloaded.text().await?, "原版下载链接");
    let head = client
        .head(format!("{base}{download}"))
        .send()
        .await?
        .error_for_status()?;
    assert_eq!(head.headers()["content-length"], "18");
    assert!(head.bytes().await?.is_empty());
    let tampered = download.replace(".txt", ".bin");
    assert_eq!(
        client
            .get(format!("{base}{tampered}"))
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .put(format!("{base}{download}"))
            .body("denied")
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let folder = format!("{base}/%E5%85%AC%E5%85%B1+%E6%96%87%E4%BB%B6/upload");
    assert_eq!(
        client
            .post(format!("{folder}?action=mkdir"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    let bytes: Vec<u8> = (0..270_000).map(|n| (n % 256) as u8).collect();
    let uploaded = client
        .put(format!("{folder}/all.bin"))
        .basic_auth("owner", Some("fixture-only"))
        .body(bytes.clone())
        .send()
        .await?;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    assert_eq!(
        tokio::fs::read(public.path().join("upload/all.bin")).await?,
        bytes
    );
    assert_eq!(
        client
            .get(format!("{folder}/all.bin"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await?
            .as_ref(),
        bytes
    );
    assert_eq!(
        client
            .delete(format!("{base}/restricted/secret.txt"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(format!("{base}/restricted/secret.txt"))
            .basic_auth("other", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(format!("{base}/restricted/%2e%2e%2fsecret.txt"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .delete(format!("{base}/restricted/"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    config.anonymous_access = true;
    server.update_config(config.clone())?;
    assert_eq!(client.get(&base).send().await?.status(), StatusCode::OK);
    assert_eq!(
        client
            .put(format!("{folder}/denied.txt"))
            .body("no")
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        client
            .delete(&folder)
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    assert!(!public.path().join("upload").exists());
    config.accounts[0].enabled = false;
    server.update_config(config)?;
    assert_eq!(
        client
            .get(format!("{base}/restricted/secret.txt"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let address = server.address;
    drop(server);
    tokio::task::yield_now().await;
    assert!(
        client
            .get(&base)
            .timeout(Duration::from_secs(1))
            .send()
            .await
            .is_err(),
        "Closed server must stop serving existing keep-alive connections"
    );
    tokio::time::timeout(Duration::from_secs(10), async {
        while tokio::net::TcpStream::connect(address).await.is_ok() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .context("HTTP服务停止后监听端口仍可连接")?;
    Ok(())
}

#[tokio::test]
async fn http_single_root_requires_auth_and_refuses_root_deletion() -> Result<()> {
    tomato_services::init_crypto();
    let root = tempfile::tempdir()?;
    let mut config = ServerConfig {
        bind_address: "127.0.0.1".into(),
        port: 0,
        root_directory: root.path().to_string_lossy().into(),
        ..Default::default()
    };
    ensure!(
        tomato_services::servers::http::start(config.clone())
            .await
            .is_err()
    );
    config.accounts.push(account("owner"));
    let server = tomato_services::servers::http::start(config).await?;
    let base = format!("http://{}", server.address);
    let client = Client::new();
    assert_eq!(
        client
            .delete(&base)
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        client
            .get(format!("{base}/missing.txt"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        client
            .get(format!("{base}/%invalid"))
            .basic_auth("owner", Some("fixture-only"))
            .send()
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(root.path().is_dir());
    Ok(())
}
