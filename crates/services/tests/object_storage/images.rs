use super::*;
use tomato_services::files::images::{Mode, Request};
#[path = "image_capture.rs"]
mod capture;

#[tokio::test]
async fn signed_image_preview_reads_real_bucket_and_prefix_and_preserves_download_bytes()
-> Result<()> {
    let state = StateData::default();
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        4,
        2,
        image::Rgba([1, 2, 3, 255]),
    ))
    .write_to(&mut png, image::ImageFormat::Png)?;
    let png = png.into_inner();
    {
        let mut objects = state.lock().unwrap();
        objects.buckets.insert("images".into());
        objects
            .files
            .insert(("images".into(), "prefix/中文.png".into()), png.clone());
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let router = axum::Router::new()
        .fallback(serve)
        .with_state(state.clone());
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    let result = async {
        for kind in ["S3", "ALIYUN_OSS"] {
            for bucket_root in [false, true] {
                let mut connection = Connection::new(kind);
                for (key, value) in [
                    ("username", "fixture-key"),
                    ("password", "fixture-secret"),
                    ("region", "us-east-1"),
                ] {
                    connection.set(key, value);
                }
                connection.set("endpoint", format!("http://{address}"));
                connection.set("pathStyleAccess", true);
                if !bucket_root {
                    connection.set("bucket", "images");
                    connection.set("s3Prefix", "prefix/");
                }
                let storage = Storage::new(&connection)?;
                let path = if bucket_root {
                    "/images/prefix/中文.png"
                } else {
                    "/中文.png"
                };
                for mode in [Mode::Thumbnail, Mode::Full] {
                    let Output::Image {
                        asset,
                        path: actual,
                        mode: actual_mode,
                    } = storage
                        .execute(Operation::ReadImage(Request::new(path.into(), mode)))
                        .await?
                    else {
                        anyhow::bail!("image reply missing")
                    };
                    assert_eq!(actual, path);
                    assert_eq!(actual_mode, mode);
                    assert_eq!((asset.width, asset.height), (4, 2));
                    assert_eq!(
                        (asset.picture.width, asset.picture.height),
                        if mode == Mode::Thumbnail {
                            (48, 24)
                        } else {
                            (4, 2)
                        }
                    );
                }
                let directory = tempfile::tempdir()?;
                let downloaded = directory.path().join("中文.png");
                storage
                    .execute(Operation::Download {
                        remote: path.into(),
                        local: downloaded.clone(),
                    })
                    .await?;
                assert_eq!(std::fs::read(downloaded)?, png);
                let cancelled = Request::new(path.into(), Mode::Full);
                cancelled.cancel.cancel();
                assert!(
                    storage
                        .execute(Operation::ReadImage(cancelled))
                        .await
                        .is_err()
                );
            }
        }
        anyhow::Ok(())
    }
    .await;
    server.abort();
    result?;
    assert!(state.lock().unwrap().signed >= 20);
    Ok(())
}

#[tokio::test]
#[ignore = "Set TOMATO_LARGE_PNG_SAMPLE for a read-only user PNG through isolated signed S3"]
async fn user_large_png_reproduces_real_s3_thumbnail_preview_markdown_and_original_download()
-> Result<()> {
    use anyhow::{Context, ensure};
    let source = std::path::PathBuf::from(
        std::env::var_os("TOMATO_LARGE_PNG_SAMPLE").context("PNG sample missing")?,
    );
    let bytes = std::fs::read(&source)?;
    let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()?
        .into_dimensions()?;
    ensure!(
        width > 8192
            || height > 8192
            || u64::from(width) * u64::from(height) * 4 > 64 * 1024 * 1024,
        "sample must reproduce the original size guards"
    );
    let state = StateData::default();
    {
        let mut objects = state.lock().unwrap();
        objects.buckets.insert("large-images".into());
        objects.files.insert(
            ("large-images".into(), "中文大图.png".into()),
            bytes.clone(),
        );
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = tokio::spawn(
        axum::serve(
            listener,
            axum::Router::new()
                .fallback(serve)
                .with_state(state.clone()),
        )
        .into_future(),
    );
    struct Server(tokio::task::JoinHandle<std::io::Result<()>>);
    impl Drop for Server {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Server(server);
    let mut connection = Connection::new("S3");
    for (key, value) in [
        ("username", "fixture-key"),
        ("password", "fixture-secret"),
        ("region", "us-east-1"),
        ("bucket", "large-images"),
    ] {
        connection.set(key, value);
    }
    connection.set("endpoint", format!("http://{address}"));
    connection.set("pathStyleAccess", true);
    let storage = Storage::new(&connection)?;
    let mut pictures = Vec::new();
    for mode in [Mode::Thumbnail, Mode::Full] {
        let Output::Image { asset, .. } = storage
            .execute(Operation::ReadImage(Request::new(
                "/中文大图.png".into(),
                mode,
            )))
            .await?
        else {
            anyhow::bail!("image reply missing")
        };
        assert_eq!((asset.width, asset.height), (width, height));
        ensure!(
            asset.picture.width <= 8192
                && asset.picture.height <= 8192
                && asset.picture.rgba.len() as u64 <= tomato_services::files::images::MAX_RGBA,
            "display canvas exceeded its budget"
        );
        pictures.push(serde_json::json!({"mode":format!("{mode:?}"),"width":asset.picture.width,"height":asset.picture.height,"rgbaBytes":asset.picture.rgba.len()}));
    }
    connection.kind = Some("LOCAL_DIRECTORY".into());
    connection.set("directoryType", "S3");
    let tomato_services::commands::Output::MarkdownImage(picture) =
        tomato_services::files::object_tree::execute(
            &connection,
            tomato_services::files::object_tree::Request::Image {
                document: "/notes/readme.md".into(),
                source: "../中文大图.png".into(),
            },
        )
        .await?
    else {
        anyhow::bail!("markdown image reply missing")
    };
    ensure!(
        picture.width <= 1600 && picture.height <= 1600,
        "markdown image exceeded display budget"
    );
    pictures.push(serde_json::json!({"mode":"Markdown","width":picture.width,"height":picture.height,"rgbaBytes":picture.rgba.len()}));
    let directory = tempfile::tempdir()?;
    let downloaded = directory.path().join("原图.png");
    storage
        .execute(Operation::Download {
            remote: "/中文大图.png".into(),
            local: downloaded.clone(),
        })
        .await?;
    ensure!(
        std::fs::read(downloaded)? == bytes && std::fs::read(&source)? == bytes,
        "source or download bytes changed"
    );
    let proof = serde_json::json!({"status":"passed","sourceBytes":bytes.len(),"sourceWidth":width,"sourceHeight":height,"pictures":pictures,"sourceUnchanged":true,"downloadBytesUnchanged":true,"savedHostsContacted":false,"signedRequests":state.lock().unwrap().signed});
    if let Some(path) = std::env::var_os("TOMATO_LARGE_PNG_PROOF") {
        std::fs::write(path, serde_json::to_vec_pretty(&proof)?)?;
    }
    println!("{proof}");
    Ok(())
}
