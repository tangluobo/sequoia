//! Independent HTTP peer verifies the tree controller and editor against real I/O.
use super::*;
use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    response::Response,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
#[derive(Default)]
struct Peer {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
    reject_writes: AtomicBool,
}
fn reply(code: u16, data: Vec<u8>, head: bool) -> Response {
    Response::builder()
        .status(code)
        .header("content-length", data.len())
        .header("last-modified", "Wed, 01 Jan 2025 00:00:00 GMT")
        .header("etag", "\"test\"")
        .body(if head {
            Body::empty()
        } else {
            Body::from(data)
        })
        .unwrap()
}
async fn serve(State(peer): State<Arc<Peer>>, request: Request) -> Response {
    if !request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|s| s.contains("fixture-key"))
    {
        return reply(
            403,
            b"<Error><Code>AccessDenied</Code></Error>".to_vec(),
            false,
        );
    }
    let method = request.method().clone();
    let head = method == axum::http::Method::HEAD;
    let key = percent_encoding::percent_decode_str(
        request
            .uri()
            .path()
            .strip_prefix("/fixture")
            .unwrap_or("/invalid")
            .trim_start_matches('/'),
    )
    .decode_utf8_lossy()
    .into_owned();
    let prefix = request
        .uri()
        .query()
        .unwrap_or("")
        .split('&')
        .find_map(|v| v.strip_prefix("prefix="))
        .map(|v| {
            percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .into_owned()
        })
        .unwrap_or_default();
    let copied = request
        .headers()
        .get("x-amz-copy-source")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            percent_encoding::percent_decode_str(v)
                .decode_utf8_lossy()
                .trim_start_matches('/')
                .strip_prefix("fixture/")
                .unwrap_or(v)
                .to_owned()
        });
    let body = to_bytes(request.into_body(), 8 * 1024 * 1024)
        .await
        .unwrap();
    let mut objects = peer.objects.lock().unwrap();
    if method == axum::http::Method::GET && key.is_empty() {
        let mut xml =
            String::from("<ListBucketResult><Name>fixture</Name><IsTruncated>false</IsTruncated>");
        let mut folders = BTreeSet::new();
        for (name, data) in objects.iter().filter(|(name, _)| name.starts_with(&prefix)) {
            if let Some((folder, _)) = name[prefix.len()..].split_once('/') {
                folders.insert(format!("{prefix}{folder}/"));
            } else {
                xml.push_str(&format!(
                    "<Contents><Key>{name}</Key><Size>{}</Size><LastModified>2025-01-01T00:00:00Z</LastModified><ETag>\"test\"</ETag></Contents>",
                    data.len()
                ));
            }
        }
        for folder in folders {
            xml.push_str(&format!(
                "<CommonPrefixes><Prefix>{folder}</Prefix></CommonPrefixes>"
            ));
        }
        xml.push_str("</ListBucketResult>");
        return reply(200, xml.into_bytes(), false);
    }
    if method == axum::http::Method::PUT {
        if peer.reject_writes.load(Ordering::Relaxed) {
            return reply(
                403,
                b"<Error><Code>AccessDenied</Code></Error>".to_vec(),
                false,
            );
        }
        let data = if let Some(source) = copied {
            objects.get(&source).cloned().unwrap()
        } else {
            body.to_vec()
        };
        objects.insert(key, data);
        return reply(200,b"<CopyObjectResult><ETag>\"test\"</ETag><LastModified>2025-01-01T00:00:00Z</LastModified></CopyObjectResult>".to_vec(),false);
    }
    if method == axum::http::Method::DELETE {
        objects.remove(&key);
        return reply(204, vec![], false);
    }
    match objects.get(&key) {
        Some(data) => reply(200, data.clone(), head),
        None => reply(404, b"<Error><Code>NoSuchKey</Code></Error>".to_vec(), head),
    }
}

#[test]
fn s3_tree_uses_runtime_credentials_and_round_trips_real_editor_save_rename_and_delete()
-> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let peer = Arc::new(Peer::default());
    peer.objects
        .lock()
        .unwrap()
        .insert("workspace/README.md".into(), "# 原文".as_bytes().to_vec());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))?;
    let address = listener.local_addr()?;
    let router = axum::Router::new().fallback(serve).with_state(peer.clone());
    let server = runtime.spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let repository = ConnectionFileRepository::new(temporary.path().join("connections.json"));
    let mut connection = Connection::new("LOCAL_DIRECTORY");
    for (key, value) in [
        ("directoryType", "S3"),
        ("bucket", "fixture"),
        ("s3Prefix", "workspace/"),
        ("username", "fixture-key"),
        ("region", "us-east-1"),
    ] {
        connection.set(key, value);
    }
    connection.set("endpoint", format!("http://{address}"));
    connection.set("pathStyleAccess", true);
    let mut store = repository.load()?;
    store.connections.push(connection.clone());
    repository.save(&store)?;
    let original = std::fs::read(repository.path())?;
    let mut app = TomatoApp::with_repository(&egui::Context::default(), runtime, repository);
    app.open_connection(connection.clone());
    assert!(app.credential.is_some());
    assert!(app.tabs.is_empty());
    let supplied = app.supplied_credentials(&connection, "fixture-secret", false)?;
    app.credential = None;
    app.open_connection(supplied);
    drain(&mut app)?;
    assert!(app.tabs.is_empty(), "directory connection must open a tree");
    assert!(app.error.is_none(), "object tree failed: {:?}", app.error);
    assert_eq!(
        app.local_trees[&connection.id].directories[std::path::Path::new("/")][0].name,
        "README.md"
    );
    app.local_tree_open(&connection, Some("/".into()), true);
    drain(&mut app)?;
    assert!(
        app.error.is_none(),
        "cached runtime secret should survive refresh: {:?}",
        app.error
    );
    let file = PathBuf::from("/README.md");
    app.open_tree_editor(connection.clone(), file.clone());
    drain(&mut app)?;
    let id = app.tabs[0].id.clone();
    assert!(
        matches!(&app.tabs[0].panel,Panel::Editor { text,markdown,.. } if text == "# 原文" && markdown.remote.is_some())
    );
    app.open_tree_editor(connection.clone(), file.clone());
    assert_eq!(app.tabs.len(), 1);
    if let Panel::Editor { text, .. } = &mut app.tabs[0].panel {
        *text = "# 保存".into();
    }
    app.handle_intent(&id, Intent::Save(file.clone(), "# 保存".into()));
    drain(&mut app)?;
    assert_eq!(
        peer.objects.lock().unwrap()["workspace/README.md"],
        "# 保存".as_bytes()
    );
    peer.reject_writes.store(true, Ordering::Relaxed);
    if let Panel::Editor { text, markdown, .. } = &mut app.tabs[0].panel {
        *text = "# 失败草稿".into();
        markdown.close_after_save = true;
    }
    app.handle_intent(&id, Intent::Save(file.clone(), "# 失败草稿".into()));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.pending.is_empty() {
        app.logic(&app.context.clone());
        anyhow::ensure!(std::time::Instant::now() < deadline, "save timeout");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(app.tabs[0].dirty());
    assert!(app.tabs[0].error.is_some());
    assert!(
        matches!(&app.tabs[0].panel,Panel::Editor { markdown,.. } if !markdown.close_after_save)
    );
    peer.reject_writes.store(false, Ordering::Relaxed);
    app.local_edit_start(connection.clone(), file.clone(), local_tree::Kind::Rename);
    app.local_edit.as_mut().unwrap().name = "改名.md".into();
    app.local_edit_submit();
    drain(&mut app)?;
    assert!(
        app.tabs.is_empty(),
        "Java S3 rename closes the old key's editor"
    );
    let renamed = PathBuf::from("/改名.md");
    app.open_tree_editor(connection.clone(), renamed.clone());
    drain(&mut app)?;
    app.local_selection
        .insert((connection.id.clone(), renamed.clone()));
    app.local_edit_start(connection.clone(), renamed, local_tree::Kind::Delete);
    app.local_edit_submit();
    drain(&mut app)?;
    assert!(app.tabs.is_empty());
    assert!(peer.objects.lock().unwrap().is_empty());
    assert_eq!(
        std::fs::read(app.repository.location().unwrap())?,
        original,
        "unremembered secret must not change old configuration"
    );
    assert!(app.store.connections[0].text("password").is_empty());
    server.abort();
    Ok(())
}

#[test]
fn sidebar_s3_inline_rename_commits_authenticated_copy_delete_and_preserves_old_name_on_failure()
-> anyhow::Result<()> {
    use crate::app::tree_rename::tests::Driver;
    let temporary = tempfile::tempdir()?;
    let peer = Arc::new(Peer::default());
    peer.objects
        .lock()
        .unwrap()
        .insert("workspace/README.md".into(), b"# original bytes".to_vec());
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let listener = runtime.block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))?;
    let address = listener.local_addr()?;
    let router = axum::Router::new().fallback(serve).with_state(peer.clone());
    let server = runtime.spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let repo = ConnectionFileRepository::new(temporary.path().join("connections.json"));
    let mut c = Connection::new("LOCAL_DIRECTORY");
    c.name = "s3_fixture".into();
    for (k, v) in [
        ("directoryType", "S3"),
        ("bucket", "fixture"),
        ("s3Prefix", "workspace/"),
        ("username", "fixture-key"),
        ("region", "us-east-1"),
    ] {
        c.set(k, v);
    }
    c.set("endpoint", format!("http://{address}"));
    c.set("pathStyleAccess", true);
    let mut store = repo.load()?;
    store.connections.push(c.clone());
    repo.save(&store)?;
    let before = std::fs::read(repo.path())?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut app = TomatoApp::with_repository(&ctx, runtime, repo);
    let supplied = app.supplied_credentials(&c, "fixture-secret", false)?;
    app.open_connection(supplied);
    app.collapsed.clear();
    drain(&mut app)?;
    let mut d = Driver::wrap(app, temporary, c);
    d.start("README.md");
    d.type_name("renamed.md");
    d.key(egui::Key::Enter);
    d.drain()?;
    d.point("renamed.md");
    {
        let objects = peer.objects.lock().unwrap();
        assert!(!objects.contains_key("workspace/README.md"));
        assert_eq!(objects["workspace/renamed.md"], b"# original bytes");
    }
    peer.reject_writes.store(true, Ordering::Relaxed);
    d.start("renamed.md");
    d.type_name("denied.md");
    d.key(egui::Key::Enter);
    d.drain()?;
    d.point("renamed.md");
    assert!(d.app.error.is_some());
    assert!(
        !peer
            .objects
            .lock()
            .unwrap()
            .contains_key("workspace/denied.md")
    );
    assert_eq!(std::fs::read(d.app.repository.location().unwrap())?, before);
    server.abort();
    Ok(())
}
