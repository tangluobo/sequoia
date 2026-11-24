//! Independent loopback S3 wire fixture: the UI must cause CopyObject, delete and relist.
use super::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
type Data = Arc<Mutex<BTreeMap<String, Vec<u8>>>>;
#[derive(Clone)]
struct State {
    data: Data,
    copies: Arc<AtomicUsize>,
    deletes: Arc<AtomicUsize>,
    deny_copy: Arc<AtomicBool>,
}
async fn serve(
    axum::extract::State(state): axum::extract::State<State>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let method = request.method().clone();
    let path = percent_encoding::percent_decode_str(request.uri().path())
        .decode_utf8()
        .unwrap()
        .into_owned();
    let key = path
        .trim_start_matches('/')
        .strip_prefix("fixture/")
        .unwrap_or("")
        .to_owned();
    let query = request
        .uri()
        .query()
        .unwrap_or("")
        .split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (
                percent_encoding::percent_decode_str(key)
                    .decode_utf8()
                    .unwrap()
                    .into_owned(),
                percent_encoding::percent_decode_str(value)
                    .decode_utf8()
                    .unwrap()
                    .into_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let copy = request.headers().get("x-amz-copy-source").map(|h| {
        percent_encoding::percent_decode_str(h.to_str().unwrap())
            .decode_utf8()
            .unwrap()
            .into_owned()
    });
    let body = axum::body::to_bytes(request.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let mut data = state.data.lock().unwrap();
    let mut status = 200;
    let response = if let Some(copy) = copy {
        if state.deny_copy.load(Ordering::Relaxed) {
            status = 403;
            b"<Error><Code>AccessDenied</Code><Message>fixture refuses copy</Message></Error>"
                .to_vec()
        } else if let Some(value) = data
            .get(
                copy.trim_start_matches('/')
                    .strip_prefix("fixture/")
                    .unwrap_or(""),
            )
            .cloned()
        {
            data.insert(key, value);
            state.copies.fetch_add(1, Ordering::Relaxed);
            b"<CopyObjectResult><LastModified>2025-01-01T00:00:00Z</LastModified><ETag>fixture-etag</ETag></CopyObjectResult>".to_vec()
        } else {
            status = 404;
            b"<Error><Code>NoSuchKey</Code></Error>".to_vec()
        }
    } else if method == axum::http::Method::GET && key.is_empty() {
        let prefix = query.get("prefix").map(String::as_str).unwrap_or("");
        let delimiter = query.get("delimiter").is_some_and(|d| d == "/");
        let mut prefixes = BTreeSet::new();
        let mut xml =
            String::from("<ListBucketResult><Name>fixture</Name><IsTruncated>false</IsTruncated>");
        for (key, value) in data.iter().filter(|(key, _)| key.starts_with(prefix)) {
            if delimiter && let Some((first, _)) = key[prefix.len()..].split_once('/') {
                prefixes.insert(format!("{prefix}{first}/"));
                continue;
            }
            xml.push_str(&format!("<Contents><Key>{key}</Key><Size>{}</Size><LastModified>2025-01-01T00:00:00Z</LastModified><ETag>fixture-etag</ETag></Contents>",value.len()));
        }
        for prefix in prefixes {
            xml.push_str(&format!(
                "<CommonPrefixes><Prefix>{prefix}</Prefix></CommonPrefixes>"
            ));
        }
        xml.push_str("</ListBucketResult>");
        xml.into_bytes()
    } else if method == axum::http::Method::PUT {
        data.insert(key, body.to_vec());
        Vec::new()
    } else if method == axum::http::Method::DELETE {
        data.remove(&key);
        state.deletes.fetch_add(1, Ordering::Relaxed);
        status = 204;
        Vec::new()
    } else if method == axum::http::Method::POST && query.contains_key("delete") {
        let text = String::from_utf8(body.to_vec()).unwrap();
        let mut xml = String::from("<DeleteResult>");
        for key in text
            .split("<Key>")
            .skip(1)
            .filter_map(|s| s.split_once("</Key>").map(|(key, _)| key))
        {
            data.remove(key);
            state.deletes.fetch_add(1, Ordering::Relaxed);
            xml.push_str(&format!("<Deleted><Key>{key}</Key></Deleted>"));
        }
        xml.push_str("</DeleteResult>");
        xml.into_bytes()
    } else if let Some(value) = data.get(&key) {
        value.clone()
    } else {
        status = 404;
        b"<Error><Code>NoSuchKey</Code></Error>".to_vec()
    };
    let length = response.len();
    let mut builder = axum::response::Response::builder()
        .status(status)
        .header("etag", "fixture-etag")
        .header("last-modified", "Wed, 01 Jan 2025 00:00:00 GMT");
    if method == axum::http::Method::HEAD {
        builder = builder.header("content-length", length);
    }
    builder
        .body(axum::body::Body::from(
            if method == axum::http::Method::HEAD {
                Vec::new()
            } else {
                response
            },
        ))
        .unwrap()
}
struct Fixture {
    state: State,
    address: std::net::SocketAddr,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    async fn start() -> anyhow::Result<Self> {
        let state = State {
            data: Arc::new(Mutex::new(BTreeMap::from([
                ("first.txt".into(), b"actual S3 file bytes".to_vec()),
                (
                    "folder/inside.txt".into(),
                    b"actual nested S3 bytes".to_vec(),
                ),
            ]))),
            copies: Default::default(),
            deletes: Default::default(),
            deny_copy: Default::default(),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let app = axum::Router::new()
            .fallback(serve)
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            state,
            address,
            server,
        })
    }
    async fn browser(&self, mode: &str) -> anyhow::Result<(Browser, Terminal)> {
        let mut b = Browser::new("S3", mode);
        b.connection
            .set("endpoint", format!("http://{}", self.address));
        b.connection.set("username", "isolated-fixture");
        b.connection.set("password", "isolated-fixture-only");
        b.connection.set("region", "us-east-1");
        b.connection.set("pathStyleAccess", true);
        let pane = b.view.panes.get_mut(&Section::Files).unwrap();
        pane.path = "/".into();
        pane.entries.clear();
        pane.refresh = true;
        let mut terminal =
            Terminal::from_file_session(tomato_services::files::session::FileSession::start(
                &tokio::runtime::Handle::current(),
                b.connection.clone(),
            ));
        wait(&mut b, &mut terminal, |b| {
            b.pane().entries.iter().any(|e| e.name == "first.txt")
        })
        .await?;
        Ok((b, terminal))
    }
}

#[tokio::test]
async fn inline_s3_rename_from_every_view_copies_deletes_and_refreshes_through_real_http()
-> anyhow::Result<()> {
    for mode in ["LIST", "ICON", "COLUMN"] {
        let fixture = Fixture::start().await?;
        let (mut b, mut terminal) = fixture.browser(mode).await?;
        b.start("first.txt");
        b.type_name("renamed:2026.txt");
        b.key(Key::Enter);
        wait(&mut b, &mut terminal, |b| {
            b.pane()
                .entries
                .iter()
                .any(|e| e.name == "renamed:2026.txt")
                && !b.view.busy(Section::Files)
        })
        .await?;
        let data = fixture.state.data.lock().unwrap();
        assert_eq!(
            data.get("renamed:2026.txt").unwrap(),
            b"actual S3 file bytes"
        );
        assert!(!data.contains_key("first.txt"));
        assert_eq!(fixture.state.copies.load(Ordering::Relaxed), 1);
        assert_eq!(fixture.state.deletes.load(Ordering::Relaxed), 1);
        assert!(b.view.dialog.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn inline_s3_open_ancestor_rename_refreshes_the_parent_and_rejection_preserves_the_old_path()
-> anyhow::Result<()> {
    for deny in [false, true] {
        let fixture = Fixture::start().await?;
        let (mut b, mut terminal) = fixture.browser("COLUMN").await?;
        b.select("folder");
        wait(&mut b, &mut terminal, |b| {
            b.pane().entries.iter().any(|e| e.name == "inside.txt")
        })
        .await?;
        let old_path = b.pane().path.clone();
        b.idle(0.5);
        let point = b.point("folder");
        b.click(point, PointerButton::Primary, Modifiers::NONE);
        b.idle(0.31);
        assert!(b.pane().rename.is_some());
        b.type_name("renamed-folder");
        fixture.state.deny_copy.store(deny, Ordering::Relaxed);
        b.key(Key::Enter);
        if deny {
            wait(&mut b, &mut terminal, |b| {
                !b.view.busy(Section::Files) && b.pane().status != "正在执行…"
            })
            .await?;
            assert_eq!(b.pane().path, old_path);
            assert!(
                b.pane().status.contains("PermissionDenied")
                    || b.pane().status.contains("AccessDenied"),
                "{}",
                b.pane().status
            );
            assert!(
                fixture
                    .state
                    .data
                    .lock()
                    .unwrap()
                    .contains_key("folder/inside.txt")
            );
            assert!(
                !fixture
                    .state
                    .data
                    .lock()
                    .unwrap()
                    .contains_key("renamed-folder/inside.txt")
            );
        } else {
            wait(&mut b, &mut terminal, |b| {
                b.pane().entries.iter().any(|e| e.name == "renamed-folder")
                    && !b.view.busy(Section::Files)
            })
            .await?;
            assert_eq!(b.pane().path, "/");
            assert!(b.pane().columns.is_empty());
            let data = fixture.state.data.lock().unwrap();
            assert_eq!(
                data.get("renamed-folder/inside.txt").unwrap(),
                b"actual nested S3 bytes"
            );
            assert!(!data.contains_key("folder/inside.txt"));
        }
        assert!(b.view.dialog.is_none());
    }
    Ok(())
}
