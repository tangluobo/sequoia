//! Explicitly authorized, read-only diagnostic for the user's saved 110 connection.
//! Ordinary test runs never contact this host or load its credentials.
use anyhow::{Context, Result, ensure};
use std::time::{Duration, Instant};
use tomato_core::Store;
use tomato_services::remote_desktop::{DesktopSession, Input, Status};

#[test]
#[ignore = "requires the user's explicit authorization and TOMATO_TEST_SAVED_110=authorized"]
fn authorized_saved_110_receives_desktop() -> Result<()> {
    ensure!(
        std::env::var("TOMATO_TEST_SAVED_110").as_deref() == Ok("authorized"),
        "Saved connection diagnostic was not explicitly enabled"
    );
    let output = std::path::PathBuf::from(
        std::env::var_os("TOMATO_TEST_SAVED_110_OUTPUT").context("Missing fresh output path")?,
    );
    ensure!(!output.exists(), "Refusing to overwrite diagnostic output");
    // Restrict diagnostics to codec/capability metadata. Do not log credentials,
    // transport payloads, clipboard contents or remote file names.
    let _ = tracing_subscriber::fmt()
        .with_env_filter("ironrdp_egfx::client=debug,ironrdp_client::rdp::graphics=debug")
        .with_test_writer()
        .without_time()
        .try_init();
    let path = Store::default_path();
    let before = std::fs::read(&path)?;
    let store = Store::load(&path)?;
    let mut connection = store
        .connections
        .iter()
        .find(|c| c.id == "902daa67-8132-4308-b81b-27f9b8a05a3a" && c.kind() == "RDP")
        .context("Authorized saved 110 connection not found")?
        .clone();
    // No Windows message loop exists in this protocol-only diagnostic. Keep the
    // user's clipboard untouched; native OLE needs a separate GUI acceptance run.
    connection.set("mapClipboard", false);
    connection.set("enableSound", false);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut session = DesktopSession::start(&connection, runtime.handle())?;
    let start = Instant::now();
    let mut first = None;
    let mut updates = 0usize;
    let mut pixels = 0usize;
    let mut canvas: Option<image::RgbaImage> = None;
    let motion = std::env::var("TOMATO_TEST_SAVED_110_MOTION").as_deref() == Ok("authorized");
    let mut moves = 0_u16;
    let mut next_move = Instant::now();
    let mut pointer_modes = std::collections::BTreeSet::new();
    while start.elapsed() < Duration::from_secs(25) {
        session.poll();
        if let Status::Failed(error) = session.status() {
            anyhow::bail!("Authorized RDP diagnostic failed: {error}");
        }
        let (_, pointer) = session.pointer();
        pointer_modes.insert((session.pointer_hidden(), pointer.as_ref().map(|p| p.size)));
        for update in session.take_rdp_updates() {
            first.get_or_insert_with(|| start.elapsed());
            updates += 1;
            let frame = update.frame;
            let (width, height) = (u32::from(frame.width), u32::from(frame.height));
            ensure!(
                width * height <= 16_777_216,
                "Unexpected desktop dimensions"
            );
            if canvas
                .as_ref()
                .is_none_or(|c| c.dimensions() != (width, height))
            {
                canvas = Some(image::RgbaImage::new(width, height));
            }
            let region = update
                .region
                .unwrap_or(tomato_services::remote_desktop::Region {
                    x: 0,
                    y: 0,
                    width: frame.width,
                    height: frame.height,
                });
            let canvas = canvas.as_mut().unwrap();
            pixels += frame.pixels.len();
            for (i, pixel) in frame.pixels.iter().enumerate() {
                let x = u32::from(region.x) + i as u32 % u32::from(region.width);
                let y = u32::from(region.y) + i as u32 / u32::from(region.width);
                canvas.put_pixel(x, y, image::Rgba(pixel.to_ne_bytes()));
            }
        }
        if motion
            && session.status() == Status::Ready
            && moves < 32
            && Instant::now() >= next_move
            && let Some(canvas) = &canvas
        {
            // Pointer motion only, through the production input path; no click,
            // keystroke, clipboard access or remote file mutation.
            let x = (canvas.width() / 2).saturating_sub(80) + u32::from(moves % 16) * 10;
            let y = canvas.height() / 2 + u32::from(moves / 16) * 24;
            session.send(Input::Pointer {
                x: x as u16,
                y: y as u16,
                button: None,
                down: false,
                wheel: 0,
            })?;
            moves += 1;
            next_move = Instant::now() + Duration::from_millis(40);
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let dispatch = session.input_dispatch_stats().unwrap_or_default();
    if motion {
        ensure!(
            moves == 32 && dispatch.messages >= 32,
            "Actual host did not receive all queued motion writes"
        );
        ensure!(
            dispatch.max_micros < 100_000,
            "Actual host input dispatch exceeded 100ms: {:?}",
            dispatch
        );
    }
    drop(session);
    ensure!(
        before == std::fs::read(&path)?,
        "Shared configuration changed externally"
    );
    let canvas = canvas.context("No desktop pixels received from saved 110")?;
    canvas.save(&output)?;
    println!(
        "saved 110: first_pixels_ms={}, updates={updates}, pixels={pixels}, dimensions={}x{}, config_unchanged=true, clipboard_tested=false",
        first.unwrap().as_millis(),
        canvas.width(),
        canvas.height()
    );
    println!(
        "input_dispatch={dispatch:?}, motion_sent={moves}, pointer_modes={pointer_modes:?}; scope=service queue to TLS completion, not physical end-to-end latency"
    );
    Ok(())
}
