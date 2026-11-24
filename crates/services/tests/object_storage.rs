//! Independent loopback REST fixture: exercise native S3/OSS HTTP paths and data.
use anyhow::Result;
use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    response::Response,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use tomato_core::Connection;
use tomato_services::files::{
    operations::{Operation, Output},
    storage::Storage,
};
#[cfg(windows)]
#[path = "support/markdown_print.rs"]
mod native_print;

const REMOTE_MARKDOWN: &str = "# S3 remote print\n\nBEFORE ![Relative](../a%20b+%E5%9B%BE.png) AFTER\n\n| A | B |\n|---|---|\n| ROOT ![Root](/root.png) END | Text |\n";
const REMOTE_KEY: &str = "/fixture/notes/README.md";

struct MarkdownBucket {
    connection: Connection,
    objects: StateData,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for MarkdownBucket {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl MarkdownBucket {
    async fn start() -> Result<Self> {
        let mut objects = Objects::default();
        objects.buckets.insert("fixture".into());
        objects.buckets.insert("other".into());
        objects.files.insert(
            ("fixture".into(), "workspace/notes/README.md".into()),
            REMOTE_MARKDOWN.as_bytes().to_vec(),
        );
        for (bucket, key, color) in [
            ("fixture", "workspace/a b+图.png", [14, 101, 190, 255]),
            ("fixture", "root.png", [13, 134, 79, 255]),
            ("other", "root.png", [230, 10, 10, 255]),
        ] {
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(56, 18, image::Rgba(color))
                .write_to(&mut png, image::ImageFormat::Png)?;
            objects
                .files
                .insert((bucket.into(), key.into()), png.into_inner());
        }
        let objects = Arc::new(Mutex::new(objects));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let router = axum::Router::new()
            .fallback(serve)
            .with_state(objects.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        let mut connection = Connection::new("S3");
        for (key, value) in [
            ("username", "fixture-key"),
            ("password", "fixture-only"),
            ("region", "us-east-1"),
            ("s3Prefix", "workspace/"),
        ] {
            connection.set(key, value);
        }
        connection.set("endpoint", endpoint);
        connection.set("pathStyleAccess", true);
        Ok(Self {
            connection,
            objects,
            task,
        })
    }
}

#[tokio::test]
async fn document_images_use_the_document_bucket_and_prefix_in_both_browser_modes() -> Result<()> {
    let fixture = MarkdownBucket::start().await?;
    for all_buckets in [false, true] {
        let mut connection = fixture.connection.clone();
        if !all_buckets {
            connection.set("bucket", "fixture");
        }
        let storage = Storage::new(&connection)?;
        let document = if all_buckets {
            REMOTE_KEY
        } else {
            "/notes/README.md"
        };
        for (source, color) in [
            (
                "../a%20b+%E5%9B%BE.png?unused#fragment",
                [14, 101, 190, 255],
            ),
            ("/root.png", [13, 134, 79, 255]),
            ("../../root.png", [13, 134, 79, 255]),
        ] {
            let Output::Image { asset, .. } = storage
                .execute(Operation::ReadDocumentImage {
                    document: document.into(),
                    source: source.into(),
                })
                .await?
            else {
                anyhow::bail!("missing image");
            };
            assert_eq!(&asset.picture.rgba[..4], &color);
        }
    }
    assert!(fixture.objects.lock().unwrap().signed >= 6);
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TOMATO_REMOTE_PRINT_EXE and a fresh TOMATO_REMOTE_PRINT_CAPTURE"]
async fn native_print_loads_remote_markdown_and_bucket_images_over_s3() -> Result<()> {
    let fixture = MarkdownBucket::start().await?;
    native_print::capture(fixture.connection.clone(), "s3-fixture").await?;
    let objects = fixture.objects.lock().unwrap();
    assert!(objects.signed >= 4);
    assert_eq!(
        &objects.files[&("fixture".into(), "workspace/notes/README.md".into())],
        REMOTE_MARKDOWN.as_bytes()
    );
    Ok(())
}

#[derive(Default)]
struct Objects {
    copy_requests: usize,
    embedded_copy_error: bool,
    fail_copy: bool,
    buckets: BTreeSet<String>,
    files: BTreeMap<(String, String), Vec<u8>>,
    signed: usize,
    uploads: BTreeMap<String, Upload>,
}
struct Upload {
    bucket: String,
    key: String,
    parts: BTreeMap<u32, Vec<u8>>,
}
#[derive(serde::Deserialize)]
struct Complete {
    #[serde(rename = "Part", default)]
    parts: Vec<Part>,
}
#[derive(serde::Deserialize)]
struct Part {
    #[serde(rename = "PartNumber")]
    number: u32,
}
type StateData = Arc<Mutex<Objects>>;
#[derive(serde::Deserialize)]
struct DeleteRequest {
    #[serde(rename = "Object", default)]
    objects: Vec<DeleteObject>,
}
#[derive(serde::Deserialize)]
struct DeleteObject {
    #[serde(rename = "Key")]
    key: String,
}
fn xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
fn response(status: u16, body: impl Into<Body>) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/xml")
        .header("etag", "\"fixture-etag\"")
        .header("last-modified", "Wed, 01 Jan 2025 00:00:00 GMT")
        .body(body.into())
        .expect("fixture response")
}
async fn serve(State(state): State<StateData>, request: Request) -> Response {
    let authorization = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    if !(authorization.starts_with("AWS4-HMAC-SHA256 ")
        || authorization.starts_with("OSS ")
        || authorization.starts_with("OSS4-HMAC-SHA256 "))
        || !authorization.contains("fixture-key")
    {
        return response(403, "Signature required");
    }
    let raw = request.uri().path();
    let Ok(path) = percent_encoding::percent_decode_str(raw).decode_utf8() else {
        return response(400, "Bad path");
    };
    let (bucket, key) = path
        .trim_start_matches('/')
        .split_once('/')
        .unwrap_or((path.trim_matches('/'), ""));
    let bucket = bucket.to_owned();
    let key = key.to_owned();
    let query: BTreeMap<_, _> =
        url::form_urlencoded::parse(request.uri().query().unwrap_or_default().as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
    let method = request.method().clone();
    let range = request
        .headers()
        .get(http::header::RANGE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let copy = request
        .headers()
        .get("x-amz-copy-source")
        .or_else(|| request.headers().get("x-oss-copy-source"))
        .and_then(|h| h.to_str().ok())
        .map(str::to_owned);
    let bytes = match to_bytes(request.into_body(), 8 * 1024 * 1024).await {
        Ok(bytes) => bytes,
        Err(_) => return response(413, "Too large"),
    };
    let mut state = state.lock().expect("fixture state");
    state.signed += 1;
    match method.as_str() {
        "POST" if query.contains_key("uploads") => {
            let id = format!("upload-{}", state.signed);
            state.uploads.insert(
                id.clone(),
                Upload {
                    bucket: bucket.clone(),
                    key: key.clone(),
                    parts: BTreeMap::new(),
                },
            );
            response(
                200,
                format!(
                    "<InitiateMultipartUploadResult><Bucket>{}</Bucket><Key>{}</Key><UploadId>{id}</UploadId></InitiateMultipartUploadResult>",
                    xml(&bucket),
                    xml(&key)
                ),
            )
        }
        "PUT" if query.contains_key("uploadId") => {
            let Some(upload) = state.uploads.get_mut(&query["uploadId"]) else {
                return response(404, "NoSuchUpload");
            };
            if upload.bucket != bucket || upload.key != key {
                return response(400, "Wrong object");
            }
            let Some(number) = query.get("partNumber").and_then(|n| n.parse::<u32>().ok()) else {
                return response(400, "Bad part");
            };
            upload.parts.insert(number, bytes.to_vec());
            response(200, "")
        }
        "POST" if query.contains_key("uploadId") => {
            let Ok(complete) = quick_xml::de::from_reader::<_, Complete>(bytes.as_ref()) else {
                return response(400, "Bad complete XML");
            };
            let Some(mut upload) = state.uploads.remove(&query["uploadId"]) else {
                return response(404, "NoSuchUpload");
            };
            if upload.bucket != bucket || upload.key != key || complete.parts.is_empty() {
                return response(400, "Wrong object");
            }
            let mut data = Vec::new();
            let mut last = 0;
            for part in complete.parts {
                if part.number <= last {
                    return response(400, "InvalidPartOrder");
                }
                last = part.number;
                let Some(bytes) = upload.parts.remove(&part.number) else {
                    return response(400, "InvalidPart");
                };
                data.extend_from_slice(&bytes);
            }
            state.files.insert((bucket.clone(), key.clone()), data);
            response(
                200,
                format!(
                    "<CompleteMultipartUploadResult><Location>fixture</Location><Bucket>{}</Bucket><Key>{}</Key><ETag>\"fixture-etag\"</ETag></CompleteMultipartUploadResult>",
                    xml(&bucket),
                    xml(&key)
                ),
            )
        }
        "DELETE" if query.contains_key("uploadId") => {
            state.uploads.remove(&query["uploadId"]);
            response(204, "")
        }
        "POST" if query.contains_key("delete") => {
            let Ok(delete) = quick_xml::de::from_reader::<_, DeleteRequest>(bytes.as_ref()) else {
                return response(400, "Bad XML");
            };
            let mut body = String::from("<DeleteResult>");
            for object in delete.objects {
                state.files.remove(&(bucket.clone(), object.key.clone()));
                body.push_str(&format!(
                    "<Deleted><Key>{}</Key></Deleted>",
                    xml(&object.key)
                ));
            }
            body.push_str("</DeleteResult>");
            response(200, body)
        }
        "GET" if bucket.is_empty() => {
            let mut body = String::from("<ListAllMyBucketsResult><Buckets>");
            for name in &state.buckets {
                body.push_str(&format!("<Bucket><Name>{}</Name><CreationDate>2025-01-01T00:00:00Z</CreationDate></Bucket>", xml(name)));
            }
            body.push_str("</Buckets><IsTruncated>false</IsTruncated></ListAllMyBucketsResult>");
            response(200, body)
        }
        "PUT" if key.is_empty() => {
            state.buckets.insert(bucket);
            response(200, "")
        }
        "DELETE" if key.is_empty() => {
            if state.files.keys().any(|(b, _)| b == &bucket) {
                response(409, "BucketNotEmpty")
            } else {
                state.buckets.remove(&bucket);
                response(204, "")
            }
        }
        "GET" if key.is_empty() => {
            let prefix = query.get("prefix").map_or("", String::as_str);
            let delimiter = query.get("delimiter").map_or("", String::as_str);
            let mut prefixes = BTreeSet::new();
            let mut body = String::from(
                "<ListBucketResult><Name>fixture</Name><IsTruncated>false</IsTruncated><KeyCount>1</KeyCount>",
            );
            for ((b, k), v) in &state.files {
                if b != &bucket || !k.starts_with(prefix) {
                    continue;
                }
                if !delimiter.is_empty()
                    && let Some((parent, _)) = k[prefix.len()..].split_once(delimiter)
                {
                    prefixes.insert(format!("{prefix}{parent}{delimiter}"));
                    continue;
                }
                body.push_str(&format!("<Contents><Key>{}</Key><LastModified>2025-01-01T00:00:00Z</LastModified><ETag>\"fixture-etag\"</ETag><Size>{}</Size><StorageClass>STANDARD</StorageClass></Contents>",xml(k),v.len()));
            }
            for prefix in prefixes {
                body.push_str(&format!(
                    "<CommonPrefixes><Prefix>{}</Prefix></CommonPrefixes>",
                    xml(&prefix)
                ));
            }
            body.push_str("</ListBucketResult>");
            response(200, body)
        }
        "PUT" => {
            if let Some(copy) = copy {
                state.copy_requests += 1;
                if state.embedded_copy_error && key.ends_with("fail.md") {
                    return response(
                        200,
                        "<Error><Code>InvalidRequest</Code><Message>fixture embedded copy error</Message></Error>",
                    );
                }
                if state.fail_copy && key.ends_with("fail.md") {
                    return response(403, "<Error><Code>AccessDenied</Code></Error>");
                }
                let Ok(copy) = percent_encoding::percent_decode_str(&copy).decode_utf8() else {
                    return response(400, "Bad copy");
                };
                let Some((b, k)) = copy.trim_start_matches('/').split_once('/') else {
                    return response(400, "Bad copy");
                };
                let Some(bytes) = state.files.get(&(b.to_owned(), k.to_owned())).cloned() else {
                    return response(404, "NoSuchKey");
                };
                state.files.insert((bucket, key), bytes);
                response(
                    200,
                    "<CopyObjectResult><LastModified>2025-01-01T00:00:00Z</LastModified><ETag>\"fixture-etag\"</ETag></CopyObjectResult>",
                )
            } else {
                state.files.insert((bucket, key), bytes.to_vec());
                response(200, "")
            }
        }
        "HEAD" | "GET" => {
            if let Some(bytes) = state.files.get(&(bucket, key)) {
                if method == http::Method::GET
                    && let Some(range) = &range
                {
                    let Some((start, end)) =
                        range.strip_prefix("bytes=").and_then(|s| s.split_once('-'))
                    else {
                        return response(400, "Bad range");
                    };
                    let Ok(start) = start.parse::<usize>() else {
                        return response(400, "Bad range");
                    };
                    let end = if end.is_empty() {
                        bytes.len().saturating_sub(1)
                    } else {
                        end.parse::<usize>()
                            .unwrap_or(usize::MAX)
                            .min(bytes.len().saturating_sub(1))
                    };
                    if start > end || start >= bytes.len() {
                        return response(416, "Outside object");
                    }
                    let mut r = response(206, bytes[start..=end].to_vec());
                    r.headers_mut().insert(
                        http::header::CONTENT_RANGE,
                        format!("bytes {start}-{end}/{}", bytes.len())
                            .parse()
                            .unwrap(),
                    );
                    r.headers_mut()
                        .insert(http::header::CONTENT_LENGTH, (end - start + 1).into());
                    return r;
                }
                let mut r = response(
                    200,
                    if method == http::Method::HEAD {
                        Body::empty()
                    } else {
                        Body::from(bytes.clone())
                    },
                );
                r.headers_mut()
                    .insert(http::header::CONTENT_LENGTH, bytes.len().into());
                r
            } else {
                response(404, "<Error><Code>NoSuchKey</Code></Error>")
            }
        }
        "DELETE" => {
            state.files.remove(&(bucket, key));
            response(204, "")
        }
        _ => response(405, "Unsupported fixture method"),
    }
}
#[path = "object_storage/copy.rs"]
mod copy;
#[path = "object_storage/documents.rs"]
mod documents;
#[path = "object_storage/images.rs"]
mod images;
#[path = "object_storage/upload.rs"]
mod upload;

#[tokio::test]
#[ignore = "requires TOMATO_OBJECT_CAPTURE_EXE and TOMATO_OBJECT_CAPTURE_PATH; native GPU window"]
async fn capture_native_object_tree_and_markdown() -> Result<()> {
    use anyhow::{Context, ensure};
    let state = StateData::default();
    {
        let mut objects = state.lock().unwrap();
        objects.buckets.insert("fixture".into());
        for (path, text) in [
            (
                "workspace/README.md",
                "# S3 目录验收\n\n中文正文，沿用原版 Markdown 工具栏。\n\n| 功能 | 状态 |\n|---|---|\n| 目录树 | 原生对象存储 |\n\n```rust\nfn main() { println!(\"hello\"); }\n```",
            ),
            ("workspace/文档/示例.md", "# 示例"),
            ("workspace/notes.txt", "text"),
        ] {
            objects
                .files
                .insert(("fixture".into(), path.into()), text.as_bytes().to_vec());
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = axum::Router::new()
        .fallback(serve)
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let mut c = Connection::new("LOCAL_DIRECTORY");
    c.name = "S3 目录验收".into();
    for (key, value) in [
        ("directoryType", "S3"),
        ("username", "fixture-key"),
        ("password", "fixture-secret"),
        ("bucket", "fixture"),
        ("s3Prefix", "workspace/"),
        ("region", "us-east-1"),
    ] {
        c.set(key, value);
    }
    c.set("endpoint", format!("http://{address}"));
    c.set("pathStyleAccess", true);
    let profile = tempfile::tempdir()?;
    tomato_core::Store {
        version: 1,
        connections: vec![c],
        ..Default::default()
    }
    .save(&profile.path().join("connections.json"))?;
    let path = std::path::PathBuf::from(
        std::env::var_os("TOMATO_OBJECT_CAPTURE_PATH").context("Missing capture path")?,
    );
    ensure!(!path.exists(), "Use a fresh capture path");
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(
        std::process::Command::new(
            std::env::var_os("TOMATO_OBJECT_CAPTURE_EXE").context("Missing capture executable")?,
        )
        .env("TOMATO_DATA_DIR", profile.path())
        .env("TOMATO_UI_CAPTURE", &path)
        .env("TOMATO_UI_CAPTURE_FRAMES", "1")
        .env("TOMATO_UI_PREVIEW", "object_tree")
        .spawn()?,
    );
    tokio::time::timeout(std::time::Duration::from_secs(60), async {
        loop {
            if let Some(status) = child.0.try_wait()? {
                ensure!(status.success(), "Window failed: {status}");
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    server.abort();
    let image = image::open(&path)?;
    ensure!(
        image.width() >= 1000 && image.height() >= 800,
        "Expected full-width native window"
    );
    ensure!(
        state.lock().unwrap().signed >= 2,
        "Tree and editor must load through S3 HTTP adapter"
    );
    println!(
        "S3 tree and Markdown captured: {} ({}x{})",
        path.display(),
        image.width(),
        image.height()
    );
    Ok(())
}

#[tokio::test]
async fn object_directory_tree_round_trips_prefix_documents_images_and_recursive_mutations()
-> Result<()> {
    use tomato_services::{
        commands::Output as Reply,
        files::{
            object_tree::{self, Request},
            tree::Action,
        },
    };
    let state = StateData::default();
    {
        let mut objects = state.lock().unwrap();
        objects.buckets.insert("fixture".into());
        for (path, body) in [
            ("workspace/README.md", "# 中文"),
            ("workspace/.hidden.md", "hidden"),
            ("outside.md", "untouched"),
        ] {
            objects
                .files
                .insert(("fixture".into(), path.into()), body.as_bytes().to_vec());
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = axum::Router::new()
        .fallback(serve)
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let mut connection = Connection::new("LOCAL_DIRECTORY");
    for (key, value) in [
        ("directoryType", "S3"),
        ("username", "fixture-key"),
        ("password", "fixture-secret"),
        ("bucket", "fixture"),
        ("s3Prefix", "workspace/"),
        ("region", "us-east-1"),
    ] {
        connection.set(key, value);
    }
    connection.set("endpoint", format!("http://{address}"));
    connection.set("pathStyleAccess", true);
    let run = |action| object_tree::execute(&connection, action);
    let Reply::Files(entries) = run(Request::List("/".into())).await? else {
        panic!("directory")
    };
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|e| e.name == ".hidden.md")); // S3 has no File.isHidden filtering.
    assert!(
        matches!(run(Request::Read("/README.md".into())).await?, Reply::Text(s) if s == "# 中文")
    );
    // Most object-store folders have no directory marker, only descendant keys.
    state.lock().unwrap().files.insert(
        ("fixture".into(), "workspace/implicit/child.md".into()),
        b"prefix-only".to_vec(),
    );
    assert!(
        run(Request::Mutate(Action::CreateDirectory {
            parent: "/".into(),
            name: "implicit".into()
        }))
        .await
        .is_err()
    );
    assert!(
        run(Request::Mutate(Action::Rename {
            path: "/README.md".into(),
            name: "implicit".into()
        }))
        .await
        .is_err()
    );
    run(Request::Mutate(Action::CreateMarkdown {
        parent: "/implicit".into(),
        name: "second".into(),
    }))
    .await?;
    run(Request::Mutate(Action::Rename {
        path: "/implicit".into(),
        name: "renamed-prefix".into(),
    }))
    .await?;
    assert!(
        matches!(run(Request::Read("/renamed-prefix/child.md".into())).await?,Reply::Text(s) if s=="prefix-only")
    );
    run(Request::Mutate(Action::Delete {
        paths: vec!["/renamed-prefix".into()],
    }))
    .await?;
    assert!(
        !state
            .lock()
            .unwrap()
            .files
            .keys()
            .any(|(_, k)| k.starts_with("workspace/implicit/")
                || k.starts_with("workspace/renamed-prefix/"))
    );
    run(Request::Mutate(Action::CreateDirectory {
        parent: "/".into(),
        name: "中文目录".into(),
    }))
    .await?;
    let Reply::LocalTree(created) = run(Request::Mutate(Action::CreateMarkdown {
        parent: "/中文目录".into(),
        name: "新文档".into(),
    }))
    .await?
    else {
        panic!("created")
    };
    let path = created.created.unwrap();
    assert!(matches!(run(Request::Read(path.clone())).await?,Reply::Text(s) if s.is_empty()));
    assert_eq!(path, std::path::PathBuf::from("/中文目录/新文档.md"));
    run(Request::Write {
        path: path.clone(),
        text: "# 保存\n![图](../preview.png)".into(),
    })
    .await?;
    assert!(
        matches!(run(Request::Read(path.clone())).await?,Reply::Text(s) if s.contains("# 保存"))
    );
    {
        use tomato_services::commands::{Command, CommandHandler, Engine};
        let Reply::Text(source) = run(Request::Read(path.clone())).await? else {
            anyhow::bail!("missing Markdown source");
        };
        let directory = tempfile::tempdir()?;
        let output = directory.path().join("新文档.md.html");
        Engine::default()
            .execute(Command::MarkdownHtml {
                source,
                title: "新文档.md".into(),
                path: output.clone(),
            })
            .await?;
        let html = tokio::fs::read_to_string(output).await?;
        assert!(html.contains("<h1>保存</h1>") && html.contains("<title>新文档.md</title>"));
    }
    assert!(
        run(Request::Mutate(Action::CreateMarkdown {
            parent: "/中文目录".into(),
            name: "新文档.md".into()
        }))
        .await
        .is_err()
    );
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::new_rgba8(4, 3).write_to(&mut png, image::ImageFormat::Png)?;
    state.lock().unwrap().files.insert(
        ("fixture".into(), "workspace/preview.png".into()),
        png.into_inner(),
    );
    assert!(
        matches!(run(Request::Image { document:path.clone(),source:"../preview.png".into() }).await?,Reply::MarkdownImage(p) if p.width == 4 && p.height == 3)
    );
    {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let data = {
            let fixture = state.lock().unwrap();
            STANDARD.encode(&fixture.files[&("fixture".into(), "workspace/preview.png".into())])
        };
        let calls = state.lock().unwrap().signed;
        assert!(matches!(run(Request::Image {
            document: path.clone(),
            source: format!("data:image/png;base64,{data}"),
        }).await?, Reply::MarkdownImage(p) if p.width == 4 && p.height == 3));
        assert_eq!(state.lock().unwrap().signed, calls);
    }
    assert!(
        run(Request::Image {
            document: path.clone(),
            source: "../../outside.png".into()
        })
        .await
        .is_err()
    );
    run(Request::Mutate(Action::Rename {
        path: path.clone(),
        name: "改名.md".into(),
    }))
    .await?;
    run(Request::Mutate(Action::CreateDirectory {
        parent: "/".into(),
        name: "目标".into(),
    }))
    .await?;
    assert!(
        run(Request::Mutate(Action::Move {
            path: "/中文目录".into(),
            parent: "/中文目录".into()
        }))
        .await
        .is_err()
    );
    run(Request::Mutate(Action::Move {
        path: "/中文目录".into(),
        parent: "/目标".into(),
    }))
    .await?;
    let moved = std::path::PathBuf::from("/目标/中文目录/改名.md");
    assert!(
        matches!(run(Request::Read(moved.clone())).await?,Reply::Text(s) if s.contains("# 保存"))
    );
    for path in ["/", "/../outside.md"] {
        assert!(
            run(Request::Mutate(Action::Delete {
                paths: vec![path.into()]
            }))
            .await
            .is_err()
        );
    }
    run(Request::Mutate(Action::Delete {
        paths: vec!["/目标".into(), moved],
    }))
    .await?;
    assert_eq!(
        state
            .lock()
            .unwrap()
            .files
            .get(&("fixture".into(), "outside.md".into()))
            .unwrap(),
        b"untouched"
    );
    assert!(
        !state
            .lock()
            .unwrap()
            .files
            .keys()
            .any(|(_, key)| key.starts_with("workspace/目标"))
    );
    // A failed copy never deletes the source tree or reports a successful move.
    {
        let mut objects = state.lock().unwrap();
        objects
            .files
            .insert(("fixture".into(), "workspace/source/".into()), vec![]);
        objects.files.insert(
            ("fixture".into(), "workspace/source/a.md".into()),
            b"first".to_vec(),
        );
        objects.files.insert(
            ("fixture".into(), "workspace/source/fail.md".into()),
            b"second".to_vec(),
        );
        objects.fail_copy = true;
    }
    assert!(
        run(Request::Mutate(Action::Rename {
            path: "/source".into(),
            name: "destination".into()
        }))
        .await
        .is_err()
    );
    assert!(
        state
            .lock()
            .unwrap()
            .files
            .contains_key(&("fixture".into(), "workspace/source/a.md".into()))
    );
    assert!(
        state
            .lock()
            .unwrap()
            .files
            .contains_key(&("fixture".into(), "workspace/source/fail.md".into()))
    );
    server.abort();
    let _ = server.await;
    Ok(())
}
#[tokio::test]
async fn s3_and_oss_bucket_object_transfer_rename_delete_use_native_signed_http() -> Result<()> {
    for kind in ["S3", "ALIYUN_OSS"] {
        let state = StateData::default();
        state
            .lock()
            .expect("fixture state")
            .buckets
            .insert("fixture".into());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let app = axum::Router::new()
            .fallback(serve)
            .with_state(state.clone());
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let mut c = Connection::new(kind);
        c.set(
            "endpoint",
            if kind == "S3" {
                address.to_string()
            } else {
                format!("http://{address}")
            },
        );
        c.set("region", "us-east-1");
        c.set("username", "fixture-key");
        c.set("password", "fixture-secret");
        c.set("pathStyleAccess", kind != "S3");
        let storage = Storage::new(&c)?;
        storage.check().await?;
        let Output::Directory { entries, .. } = storage.list("/").await? else {
            anyhow::bail!("No buckets");
        };
        assert_eq!(entries[0].name, "fixture");
        storage.execute(Operation::Mkdir("/created".into())).await?;
        storage
            .execute(Operation::Remove {
                path: "/created".into(),
                directory: true,
            })
            .await?;
        c.set("bucket", "fixture");
        let storage = Storage::new(&c)?;
        storage.execute(Operation::Mkdir("/目录".into())).await?;
        storage
            .execute(Operation::WriteText {
                path: "/目录/中文.txt".into(),
                text: "native 原生".into(),
            })
            .await?;
        let Output::Directory { entries, .. } = storage.list("/目录/").await? else {
            anyhow::bail!("No objects");
        };
        assert!(entries.iter().any(|entry| entry.name == "中文.txt"));
        storage
            .execute(Operation::Rename {
                from: "/目录/中文.txt".into(),
                to: "/目录/renamed.txt".into(),
            })
            .await?;
        let local = tempfile::tempdir()?;
        let path = local.path().join("download.txt");
        storage
            .execute(Operation::Download {
                remote: "/目录/renamed.txt".into(),
                local: path.clone(),
            })
            .await?;
        assert_eq!(tokio::fs::read_to_string(&path).await?, "native 原生");
        let selection = tomato_services::files::export::Request::selection(vec![
            tomato_services::files::export::Item::new("/目录/renamed.txt".into(), None, false),
            tomato_services::files::export::Item::new("/目录".into(), None, true),
        ])?;
        let copying = storage.execute(Operation::ExportFile(selection.clone()));
        deferred_export::assert_after_drop(
            selection,
            copying,
            "native 原生".as_bytes(),
            "/目录/renamed.txt",
        )
        .await?;
        storage
            .execute(Operation::Upload {
                local: path,
                remote: "/目录/uploaded.txt".into(),
            })
            .await?;
        assert_eq!(
            state.lock().expect("fixture state").files
                [&("fixture".into(), "目录/uploaded.txt".into())],
            "native 原生".as_bytes()
        );
        let binary: Vec<_> = (0..270_000).map(|n| (n % 256) as u8).collect();
        let source = local.path().join("binary.bin");
        tokio::fs::write(&source, &binary).await?;
        storage
            .execute(Operation::Upload {
                local: source,
                remote: "/目录/大文件.bin".into(),
            })
            .await?;
        storage
            .execute(Operation::Mkdir("/目录/子目录".into()))
            .await?;
        storage
            .execute(Operation::WriteText {
                path: "/目录/子目录/嵌套.txt".into(),
                text: "nested 中文".into(),
            })
            .await?;
        storage
            .execute(Operation::Rename {
                from: "/目录".into(),
                to: "/改名".into(),
            })
            .await?;
        let downloaded = local.path().join("large-download.bin");
        storage
            .execute(Operation::Download {
                remote: "/改名/大文件.bin".into(),
                local: downloaded.clone(),
            })
            .await?;
        assert_eq!(tokio::fs::read(downloaded).await?, binary);
        let Output::Text { text, .. } = storage
            .execute(Operation::ReadText("/改名/子目录/嵌套.txt".into()))
            .await?
        else {
            anyhow::bail!("No nested object")
        };
        assert_eq!(text, "nested 中文");
        assert!(
            !state
                .lock()
                .expect("fixture state")
                .files
                .keys()
                .any(|(_, key)| key.starts_with("目录/"))
        );
        storage
            .execute(Operation::Remove {
                path: "/改名/".into(),
                directory: true,
            })
            .await?;
        assert!(state.lock().expect("fixture state").files.is_empty());
        assert!(state.lock().expect("fixture state").uploads.is_empty());
        assert!(state.lock().expect("fixture state").signed >= 10);
        server.abort();
        let _ = server.await;
    }
    Ok(())
}
#[path = "support/deferred_export.rs"]
mod deferred_export;
