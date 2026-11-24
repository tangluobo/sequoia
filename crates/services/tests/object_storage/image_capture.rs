use super::*;
use anyhow::Context;

#[tokio::test]
#[ignore = "Set TOMATO_IMAGE_CAPTURE_EXE and TOMATO_IMAGE_CAPTURE_PATH; own native GPU child viewport"]
async fn native_image_preview_reads_signed_object_and_preserves_old_configuration() -> Result<()> {
    let executable = std::env::var_os("TOMATO_IMAGE_CAPTURE_EXE")
        .ok_or_else(|| anyhow::anyhow!("image executable missing"))?;
    let image = std::path::PathBuf::from(
        std::env::var_os("TOMATO_IMAGE_CAPTURE_PATH")
            .ok_or_else(|| anyhow::anyhow!("image capture path missing"))?,
    );
    let state = StateData::default();
    let pixels = image::RgbaImage::from_fn(1200, 800, |x, _| {
        if x / 100 % 2 == 0 {
            image::Rgba([70, 130, 180, 255])
        } else {
            image::Rgba([236, 168, 85, 255])
        }
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels).write_to(&mut bytes, image::ImageFormat::Png)?;
    {
        let mut objects = state.lock().unwrap();
        objects.buckets.insert("tomato-image-fixture".into());
        for name in ["中文.png", "预览2.png"] {
            objects.files.insert(
                ("tomato-image-fixture".into(), name.into()),
                bytes.get_ref().clone(),
            );
        }
        if std::env::var("TOMATO_IMAGE_BROWSER_MARKDOWN").as_deref() == Ok("1") {
            objects.files.insert(
                ("tomato-image-fixture".into(), "readme.md".into()),
                "# Markdown 实际文件\n\n通过原连接读取的中文正文。\n\n![相对图片](中文.png)"
                    .as_bytes()
                    .to_vec(),
            );
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
    struct Task(tokio::task::JoinHandle<()>);
    impl Drop for Task {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    let _server = Task(server);
    let profile = tempfile::Builder::new()
        .prefix("tomato-image-fixture-")
        .tempdir()?;
    let mut connection = Connection::new("S3");
    connection.name = "图片预览验收".into();
    for (key, value) in [
        ("bucket", "tomato-image-fixture"),
        ("region", "us-east-1"),
        ("username", "fixture-key"),
        ("password", "fixture-secret"),
    ] {
        connection.set(key, value);
    }
    connection.set("endpoint", format!("http://{address}"));
    connection.set("pathStyleAccess", true);
    let config = profile.path().join("connections.json");
    tomato_core::Store {
        connections: vec![connection],
        ..Default::default()
    }
    .save(&config)?;
    let own_before = std::fs::read(&config)?;
    let shared = std::env::var_os("USERPROFILE")
        .map(std::path::PathBuf::from)
        .map(|path| path.join(".tomato/connections.json"));
    let before = shared.as_ref().and_then(|path| std::fs::read(path).ok());
    let mut command = std::process::Command::new(executable);
    command
        .env("TOMATO_DATA_DIR", profile.path())
        .env("TOMATO_UI_CAPTURE", &image)
        .env("TOMATO_UI_CAPTURE_FRAMES", "4")
        .env("TOMATO_UI_PREVIEW", "image_preview")
        .env("WGPU_BACKEND", "vulkan");
    command
        .stdout(std::fs::File::create(image.with_extension("stdout.log"))?)
        .stderr(std::fs::File::create(image.with_extension("stderr.log"))?);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            if self.0.try_wait().ok().flatten().is_none() {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
    }
    let mut child = Child(command.spawn()?);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(45);
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        anyhow::ensure!(
            std::time::Instant::now() < end,
            "own image capture timed out"
        );
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    anyhow::ensure!(status.success(), "own image process failed");
    if std::env::var_os("TOMATO_IMAGE_BROWSER_MODE").is_some() {
        if std::env::var("TOMATO_IMAGE_BROWSER_MENU").as_deref() == Ok("1")
            || std::env::var("TOMATO_IMAGE_BROWSER_MARKDOWN").as_deref() == Ok("1")
        {
            let trace: serde_json::Value =
                serde_json::from_slice(&std::fs::read(image.with_extension("browser.json"))?)?;
            let markdown = std::env::var("TOMATO_IMAGE_BROWSER_MARKDOWN").as_deref() == Ok("1");
            anyhow::ensure!(
                trace["ready"] == true
                    && trace["markdown"] == markdown
                    && trace["popup"] == !markdown
                    && trace["osInputUsed"] == false,
                "native browser controls incomplete"
            );
            anyhow::ensure!(
                trace["syntheticPointerEvents"] == if markdown { 4 } else { 2 },
                "native gesture incomplete"
            );
        }
        if std::env::var("TOMATO_IMAGE_BROWSER_RENAME").as_deref() == Ok("1") {
            let rename: serde_json::Value =
                serde_json::from_slice(&std::fs::read(image.with_extension("rename.json"))?)?;
            anyhow::ensure!(
                rename["inline"] == true && rename["focused"] == true && rename["popup"] == false,
                "name editor must be in place and focused"
            );
            anyhow::ensure!(
                rename["name"] == "中文.png"
                    && rename["syntheticPointerEvents"] == 4
                    && rename["osInputUsed"] == false,
                "selected-then-click native input failed"
            );
            let field = rename["field"]
                .as_array()
                .context("rename rectangle missing")?;
            let coordinate = |i: usize| field[i].as_f64().unwrap();
            let point = rename["target"]
                .as_array()
                .context("original name coordinate missing")?;
            anyhow::ensure!(
                coordinate(3) > 10. && coordinate(3) <= 26.1 && coordinate(2) > 50.,
                "name field changed row height or wrapped"
            );
            anyhow::ensure!(
                point[0].as_f64().unwrap() >= coordinate(0)
                    && point[0].as_f64().unwrap() <= coordinate(0) + coordinate(2)
                    && point[1].as_f64().unwrap() >= coordinate(1)
                    && point[1].as_f64().unwrap() <= coordinate(1) + coordinate(3),
                "name editor moved away from the clicked label"
            );
        }
        let captured = image::open(&image)?;
        anyhow::ensure!(
            captured.width() > 600 && captured.height() > 400,
            "browser framebuffer missing"
        );
        anyhow::ensure!(
            state.lock().unwrap().signed >= 1,
            "browser did not read the signed file listing"
        );
        anyhow::ensure!(
            std::fs::read(config)? == own_before,
            "fixture config changed"
        );
        if let Some(shared) = shared {
            anyhow::ensure!(
                std::fs::read(shared).ok() == before,
                "shared configuration changed"
            );
        }
        println!("Native file browser captured: {}", image.display());
        return Ok(());
    }
    let trace: serde_json::Value =
        serde_json::from_slice(&std::fs::read(image.with_extension("state.json"))?)?;
    assert_eq!(trace["ready"], true);
    assert_eq!(trace["width"].as_f64(), Some(1200.));
    assert_eq!(trace["height"].as_f64(), Some(800.));
    assert_eq!(trace["clientWidth"].as_f64(), Some(784.));
    assert_eq!(trace["clientHeight"].as_f64(), Some(561.));
    anyhow::ensure!(
        (trace["scale"].as_f64().unwrap() - 0.625).abs() < 0.001,
        "initial fit differs from original"
    );
    assert_eq!(trace["error"], "");
    assert!(state.lock().unwrap().signed >= 4);
    anyhow::ensure!(
        std::fs::read(config)? == own_before,
        "fixture config changed"
    );
    if let Some(shared) = shared {
        anyhow::ensure!(
            std::fs::read(shared).ok() == before,
            "shared configuration changed"
        );
    }
    println!("Native image state: {trace}");
    Ok(())
}
