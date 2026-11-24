use super::*;
#[path = "image_tests.rs"]
mod images;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tomato_services::files::{
    clipboard::{Content, Repository},
    object_copy::{self, Selection},
    session::FileSession,
};
#[derive(Default)]
struct Clipboard {
    value: Mutex<Option<Content>>,
    reads: AtomicUsize,
    writes: AtomicUsize,
}
impl Repository for Clipboard {
    fn read(&self) -> anyhow::Result<Content> {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.value
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| anyhow::anyhow!("fixture clipboard empty"))
    }
    fn write_objects(&self, selection: &Selection) -> anyhow::Result<()> {
        self.writes.fetch_add(1, Ordering::Relaxed);
        *self.value.lock().unwrap() = Some(Content::Objects(selection.clone()));
        Ok(())
    }
}
struct Lookup(tomato_core::Connection);
impl object_copy::Connections for Lookup {
    fn find(&self, id: &str) -> anyhow::Result<tomato_core::Connection> {
        anyhow::ensure!(id == self.0.id, "fixture source id mismatch");
        Ok(self.0.clone())
    }
}
type Data = Arc<Mutex<BTreeMap<(String, String), Vec<u8>>>>;
async fn serve(
    axum::extract::State(data): axum::extract::State<Data>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let method = request.method().clone();
    let path = percent_encoding::percent_decode_str(request.uri().path())
        .decode_utf8()
        .unwrap()
        .into_owned();
    let (bucket, key) = path
        .trim_start_matches('/')
        .split_once('/')
        .unwrap_or((path.trim_matches('/'), ""));
    let (bucket, key) = (bucket.to_owned(), key.to_owned());
    let body = axum::body::to_bytes(request.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let mut data = data.lock().unwrap();
    let mut status = 200;
    let mut length = None;
    let body = if method == axum::http::Method::GET && key.is_empty() {
        let mut xml =
            String::from("<ListBucketResult><Name>fixture</Name><IsTruncated>false</IsTruncated>");
        for ((_, k), v) in data.iter().filter(|((b, _), _)| b == &bucket) {
            xml.push_str(&format!("<Contents><Key>{k}</Key><Size>{}</Size><LastModified>2025-01-01T00:00:00Z</LastModified><ETag>fixture-etag</ETag></Contents>",v.len()));
        }
        xml.push_str("</ListBucketResult>");
        xml.into_bytes()
    } else if method == axum::http::Method::DELETE {
        data.remove(&(bucket, key));
        status = 204;
        Vec::new()
    } else if method == axum::http::Method::PUT {
        data.insert((bucket, key), body.to_vec());
        Vec::new()
    } else if let Some(value) = data.get(&(bucket, key)) {
        length = Some(value.len());
        if method == axum::http::Method::HEAD {
            Vec::new()
        } else {
            value.clone()
        }
    } else {
        status = 404;
        b"<Error><Code>NoSuchKey</Code></Error>".to_vec()
    };
    let mut response = axum::response::Response::builder().status(status);
    if let Some(length) = length {
        response = response.header("content-length", length);
    }
    response.body(axum::body::Body::from(body)).unwrap()
}
struct Fixture {
    data: Data,
    server: tokio::task::JoinHandle<()>,
    address: std::net::SocketAddr,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    async fn start() -> anyhow::Result<Self> {
        let data = Data::default();
        data.lock().unwrap().insert(
            ("source".into(), "example.txt".into()),
            b"actual native copy from keyboard".to_vec(),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let router = axum::Router::new().fallback(serve).with_state(data.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Ok(Self {
            data,
            server,
            address,
        })
    }
    fn connection(&self, bucket: &str) -> tomato_core::Connection {
        let mut c = tomato_core::Connection::new("S3");
        c.set("endpoint", format!("http://{}", self.address));
        c.set("username", "fixture-key");
        c.set("password", "fixture-only");
        c.set("region", "us-east-1");
        c.set("pathStyleAccess", true);
        c.set("bucket", bucket);
        c
    }
}
fn frame(
    ctx: &egui::Context,
    view: &mut Workspace,
    c: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Pos2)> {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 800.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            render(ui, view, c, icons);
        },
    );
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(t) = &shape.shape {
                Some((t.galley.job.text.clone(), t.pos + t.galley.size() / 2.))
            } else {
                None
            }
        })
        .collect();
    output.textures_delta.clear();
    texts
}
fn click(
    ctx: &egui::Context,
    view: &mut Workspace,
    c: &tomato_core::Connection,
    icons: &mut crate::icons::Icons,
    point: egui::Pos2,
) {
    for pressed in [true, false] {
        frame(
            ctx,
            view,
            c,
            icons,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}
async fn wait(
    view: &mut Workspace,
    terminal: &mut tomato_services::terminal::Terminal,
    ctx: &egui::Context,
    predicate: impl Fn(&Workspace) -> bool,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        view.poll(terminal, ctx, true);
        if predicate(view) {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "file controller timed out: {}",
            view.panes
                .get(&Section::Files)
                .map(|p| p.status.as_str())
                .unwrap_or("no pane")
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
#[tokio::test]
async fn object_address_edit_is_a_draft_and_enter_navigates_without_opening_the_selected_file()
-> anyhow::Result<()> {
    let fixture = Fixture::start().await?;
    let connection = fixture.connection("source");
    let mut terminal = tomato_services::terminal::Terminal::from_file_session(FileSession::start(
        &tokio::runtime::Handle::current(),
        connection.clone(),
    ));
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut icons = crate::icons::Icons::default();
    let mut view = Workspace::standalone_files("LIST", "/");
    wait(&mut view, &mut terminal, &ctx, |v| {
        v.panes[&Section::Files].entries.len() == 1
    })
    .await?;
    let texts = frame(&ctx, &mut view, &connection, &mut icons, vec![]);
    let address = texts
        .iter()
        .find(|(s, _)| s == "/")
        .expect("address field")
        .1;
    let file = texts
        .iter()
        .find(|(s, _)| s == "example.txt")
        .expect("actual file row")
        .1;
    click(&ctx, &mut view, &connection, &mut icons, address);
    frame(
        &ctx,
        &mut view,
        &connection,
        &mut icons,
        vec![egui::Event::Text("not-a-real-bucket".into())],
    );
    assert_eq!(view.panes[&Section::Files].path, "/");
    assert_eq!(
        view.panes[&Section::Files].path_input.as_deref(),
        Some("not-a-real-bucket")
    );
    click(&ctx, &mut view, &connection, &mut icons, file);
    assert!(view.panes[&Section::Files].path_input.is_none());
    assert_eq!(view.panes[&Section::Files].path, "/");
    assert_eq!(view.panes[&Section::Files].selected.len(), 1);
    click(&ctx, &mut view, &connection, &mut icons, address);
    frame(
        &ctx,
        &mut view,
        &connection,
        &mut icons,
        vec![egui::Event::Text(" / ".into())],
    );
    frame(
        &ctx,
        &mut view,
        &connection,
        &mut icons,
        vec![egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    wait(&mut view, &mut terminal, &ctx, |v| {
        v.panes[&Section::Files].selected.is_empty() && !v.busy(Section::Files)
    })
    .await?;
    assert_eq!(view.panes[&Section::Files].path, "/");
    assert!(
        view.editor.is_none(),
        "address Enter must not also open the selected file"
    );
    terminal.stop_file_session().await;
    Ok(())
}
#[tokio::test]
async fn keyboard_object_copy_and_paste_run_the_real_actor_and_keep_completion_after_refresh_without_double_paste()
-> anyhow::Result<()> {
    let fixture = Fixture::start().await?;
    let source = fixture.connection("source");
    let target = fixture.connection("target");
    let clipboard = Arc::new(Clipboard::default());
    let context = object_copy::Context {
        clipboard: clipboard.clone(),
        connections: Arc::new(Lookup(source.clone())),
    };
    let runtime = tokio::runtime::Handle::current();
    let mut source_terminal = tomato_services::terminal::Terminal::from_file_session(
        FileSession::start_with_context(&runtime, source.clone(), context.clone()),
    );
    let mut target_terminal = tomato_services::terminal::Terminal::from_file_session(
        FileSession::start_with_context(&runtime, target.clone(), context),
    );
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut icons = crate::icons::Icons::default();
    let make_view = || Workspace::standalone_files("LIST", "/");
    let mut source_view = make_view();
    let mut target_view = make_view();
    wait(&mut source_view, &mut source_terminal, &ctx, |v| {
        v.panes[&Section::Files].entries.len() == 1
    })
    .await?;
    frame(&ctx, &mut source_view, &source, &mut icons, vec![]);
    let texts = frame(&ctx, &mut source_view, &source, &mut icons, vec![]);
    let point = texts
        .iter()
        .find(|(text, _)| text == "example.txt")
        .expect("actual file row must be drawn")
        .1;
    click(&ctx, &mut source_view, &source, &mut icons, point);
    frame(
        &ctx,
        &mut source_view,
        &source,
        &mut icons,
        vec![egui::Event::Copy],
    );
    wait(&mut source_view, &mut source_terminal, &ctx, |v| {
        v.panes[&Section::Files].status.starts_with("已复制")
    })
    .await?;
    assert_eq!(clipboard.writes.load(Ordering::Relaxed), 1);
    wait(&mut target_view, &mut target_terminal, &ctx, |v| {
        v.file_status == tomato_services::terminal::Status::Ready && !v.busy(Section::Files)
    })
    .await?;
    frame(&ctx, &mut target_view, &target, &mut icons, vec![]);
    let texts = frame(&ctx, &mut target_view, &target, &mut icons, vec![]);
    let point = texts
        .iter()
        .find(|(text, _)| text == "空目录")
        .expect("target directory must be empty")
        .1;
    click(&ctx, &mut target_view, &target, &mut icons, point);
    frame(
        &ctx,
        &mut target_view,
        &target,
        &mut icons,
        vec![egui::Event::Paste("1 个文件".into())],
    );
    wait(&mut target_view, &mut target_terminal, &ctx, |v| {
        v.panes[&Section::Files].entries.len() == 1
            && v.panes[&Section::Files].status == "粘贴完成: 成功 1 个"
            && !v.busy(Section::Files)
    })
    .await?;
    frame(
        &ctx,
        &mut target_view,
        &target,
        &mut icons,
        vec![egui::Event::Key {
            key: egui::Key::V,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
    );
    target_view.poll(&mut target_terminal, &ctx, true);
    assert_eq!(
        clipboard.reads.load(Ordering::Relaxed),
        1,
        "release after a fast completed paste must not run it again"
    );
    assert!(target_view.object_copy.is_none());
    assert_eq!(
        fixture.data.lock().unwrap()[&("target".into(), "example.txt".into())],
        b"actual native copy from keyboard"
    );
    frame(
        &ctx,
        &mut target_view,
        &target,
        &mut icons,
        vec![egui::Event::Key {
            key: egui::Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        }],
    );
    wait(&mut target_view, &mut target_terminal, &ctx, |v| {
        v.object_copy.is_none()
            && !v.busy(Section::Files)
            && v.panes[&Section::Files].status == "粘贴完成: 成功 1 个"
    })
    .await?;
    frame(
        &ctx,
        &mut target_view,
        &target,
        &mut icons,
        vec![
            egui::Event::ModifiersChanged(egui::Modifiers::NONE),
            egui::Event::Key {
                key: egui::Key::V,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    target_view.poll(&mut target_terminal, &ctx, true);
    assert_eq!(
        clipboard.reads.load(Ordering::Relaxed),
        2,
        "raw shortcut also runs once when Ctrl is released first"
    );
    click(
        &ctx,
        &mut target_view,
        &target,
        &mut icons,
        egui::pos2(40., 15.),
    );
    frame(
        &ctx,
        &mut target_view,
        &target,
        &mut icons,
        vec![egui::Event::Copy],
    );
    target_view.poll(&mut target_terminal, &ctx, true);
    assert_eq!(
        clipboard.writes.load(Ordering::Relaxed),
        1,
        "copy in the path field must not stage selected objects"
    );
    target_view.paste_objects(&ctx, "/".into());
    let pending = target_view.object_copy.as_ref().unwrap().0.clone();
    drop(target_view);
    assert!(
        pending.is_cancelled(),
        "closing a workspace cancels a queued copy while worker references exist"
    );
    Ok(())
}
