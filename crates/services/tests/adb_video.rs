//! Actual scrcpy sockets and H.264 reference frames through the public desktop actor.
#[path = "support/adb_peer.rs"]
mod adb_peer;
#[path = "adb_video/pool.rs"]
mod pool;
use adb_peer::Fixture;

use anyhow::{Context, Result, ensure};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use tomato_services::remote_desktop::{DesktopSession, Input, Status};

async fn frame(session: &mut DesktopSession) -> Result<tomato_services::remote_desktop::RgbaFrame> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            session.poll();
            if let Some(frame) = session.take_rgba_frame() {
                return Ok(frame);
            }
            if let Status::Failed(error) = session.status() {
                anyhow::bail!(error);
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .context("H.264 frame timeout")?
}
fn write_video(exe: &Path, width: usize, height: usize) -> Result<()> {
    use openh264::{
        OpenH264API,
        encoder::{BitRate, Encoder, EncoderConfig, FrameRate},
        formats::YUVBuffer,
    };
    let config = EncoderConfig::new()
        .max_frame_rate(FrameRate::from_hz(60.0))
        .bitrate(BitRate::from_bps(8_000_000))
        .skip_frames(false);
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config)?;
    let mut stream = b"h264".to_vec();
    stream.extend((width as u32).to_be_bytes());
    stream.extend((height as u32).to_be_bytes());
    for n in 0..90 {
        if n > 0 && n % 30 == 0 {
            encoder.force_intra_frame();
        }
        let mut pixels = vec![128; width * height * 3 / 2];
        pixels[..width * height].fill(40);
        let x = (n * 16) % (width - width / 6);
        for y in height / 4..height / 2 {
            pixels[y * width + x..y * width + x + width / 6].fill(180);
        }
        if std::env::var("TOMATO_ADB_VIDEO_PATTERN").as_deref() == Ok("full") {
            pixels[..width * height].fill((32 + n * 11 % 180) as u8);
            pixels[width * height..width * height * 5 / 4].fill((16 + n * 7 % 220) as u8);
            pixels[width * height * 5 / 4..].fill((16 + n * 17 % 220) as u8);
        }
        let image = YUVBuffer::from_vec(pixels, width, height);
        let encoded = encoder.encode(&image)?.to_vec();
        let mut config = vec![];
        let mut payload = vec![];
        let mut key = false;
        for nal in openh264::nal_units(&encoded) {
            let prefix = if nal.starts_with(&[0, 0, 0, 1]) { 4 } else { 3 };
            let kind = nal[prefix] & 31;
            if matches!(kind, 7 | 8) {
                config.extend_from_slice(nal);
            } else {
                key |= kind == 5;
                payload.extend_from_slice(nal);
            }
        }
        if !config.is_empty() {
            stream.extend((1u64 << 63).to_be_bytes());
            stream.extend((config.len() as u32).to_be_bytes());
            stream.extend(config);
        }
        ensure!(
            !payload.is_empty(),
            "Encoder skipped an owned fixture frame"
        );
        stream.extend(((if key { 1u64 << 62 } else { 0 }) | ((n as u64) * 16_667)).to_be_bytes());
        stream.extend((payload.len() as u32).to_be_bytes());
        stream.extend(payload);
    }
    std::fs::write(exe.with_extension("video.scrcpy"), stream)?;
    if let Some(path) = std::env::var_os("TOMATO_ADB_VIDEO_EXPORT") {
        std::fs::copy(exe.with_extension("video.scrcpy"), path)?;
    }
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the owned adb_fixture example; uses actual compressed 1080p frames"]
async fn native_h264_1080p_video_live_touch_latest_frame_and_owned_cleanup() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let exe = Fixture::start(temp.path())?;
    write_video(&exe, 1920, 1080)?;
    let mut session = DesktopSession::start(&exe.connection(), &tokio::runtime::Handle::current())?;
    let notify = std::sync::Arc::new(tokio::sync::Notify::new());
    let repaint = notify.clone();
    session.on_output(move || repaint.notify_one());
    let first = frame(&mut session).await?;
    assert_eq!((first.width, first.height), (1920, 1080));
    session.send(Input::Resize {
        width: 1920,
        height: 1080,
    })?;
    session.send(Input::Pointer {
        x: 10,
        y: 20,
        button: Some(1),
        down: true,
        wheel: 0,
    })?;
    session.send(Input::Pointer {
        x: 30,
        y: 40,
        button: None,
        down: false,
        wheel: 0,
    })?;
    let input_log = exe.with_extension("scrcpy-input.jsonl");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let events = std::fs::read_to_string(&input_log)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str::<Vec<u8>>(l).ok())
                .collect::<Vec<_>>();
            if let Some(m) = events.iter().find(|m| m.starts_with(&[2, 2])) {
                assert!(events.iter().any(|m| m.starts_with(&[2, 0])));
                assert!(!events.iter().any(|m| m.starts_with(&[2, 1])));
                assert_eq!(u16::from_be_bytes(m[18..20].try_into().unwrap()), 1920);
                break;
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await?;
    session.send(Input::Pointer {
        x: 30,
        y: 40,
        button: Some(1),
        down: false,
        wheel: 0,
    })?;
    let started = Instant::now();
    let mut count = 0;
    let mut gaps = vec![];
    let mut decode = vec![];
    let mut previous = Instant::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        tokio::select! { _ = notify.notified() => (), _ = tokio::time::sleep_until(deadline) => break }
        session.poll();
        if let Some(f) = session.take_rgba_frame() {
            assert_eq!(f.pixels.len(), 1920 * 1080);
            count += 1;
            gaps.push(previous.elapsed().as_secs_f64() * 1000.);
            previous = Instant::now();
            decode.push(session.adb_video_stats().unwrap().last_decode_micros);
        }
    }
    let fps = count as f64 / started.elapsed().as_secs_f64();
    gaps.sort_by(f64::total_cmp);
    decode.sort_unstable();
    ensure!(count > 30, "H.264 flow stalled: {count}");
    eprintln!(
        "ADB owned 1920x1080/60 source: received {fps:.2} fps; inter-frame median {:.2} ms / p95 {:.2} ms; decode+RGBA p95 {:.3} ms",
        gaps[gaps.len() / 2],
        gaps[gaps.len() * 95 / 100],
        decode[decode.len() * 95 / 100] as f64 / 1000.
    );
    tokio::time::sleep(Duration::from_millis(600)).await;
    let resumed = Instant::now();
    frame(&mut session).await?;
    let emitted = std::fs::read_to_string(exe.with_extension("h264-heartbeat"))?
        .parse::<u64>()
        .unwrap_or(0);
    let stats = session.adb_video_stats().unwrap();
    ensure!(
        stats.decoded_frames + 8 >= emitted,
        "Old frames replayed after consumer resumed: decoded {} / emitted {}",
        stats.decoded_frames,
        emitted
    );
    eprintln!(
        "ADB stalled consumer resume: {:.2} ms; decoded {}, emitted {}, backlog resets {}",
        resumed.elapsed().as_secs_f64() * 1000.,
        stats.decoded_frames,
        emitted,
        stats.queue_resets
    );
    let commands = std::fs::read_to_string(exe.with_extension("commands.jsonl"))?;
    ensure!(
        commands.contains("max_fps=60") && !commands.contains("screencap"),
        "Video silently used PNG"
    );
    drop(session);
    tokio::time::timeout(Duration::from_secs(8), async {
        while !exe.with_extension("server-removed").exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    let before = std::fs::read(exe.with_extension("h264-heartbeat"))?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(before, std::fs::read(exe.with_extension("h264-heartbeat"))?);
    let commands = std::fs::read_to_string(exe.with_extension("commands.jsonl"))?;
    assert!(commands.contains(r#"["remove","/data/local/tmp/tomato-"#));
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires the owned adb_fixture example"]
async fn rejected_video_bootstrap_falls_back_to_the_existing_png_adapter() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let exe = Fixture::start(temp.path())?;
    std::fs::write(exe.with_extension("reject-scrcpy"), b"owned")?;
    let mut session = DesktopSession::start(&exe.connection(), &tokio::runtime::Handle::current())?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            session.poll();
            if let Some(f) = session.take_frame() {
                assert_eq!((f.width, f.height), (96, 64));
                break;
            }
            if let Status::Failed(e) = session.status() {
                anyhow::bail!(e);
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    assert_eq!(session.adb_video_stats().unwrap().mode, "PNG");
    drop(session);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned adb_fixture; genuine 1080p I420 transport for GPU presentation"]
async fn native_i420_1080p_keeps_planes_packed_and_releases_the_actor() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let exe = Fixture::start(temp.path())?;
    write_video(&exe, 1920, 1080)?;
    let mut session = DesktopSession::start(&exe.connection(), &tokio::runtime::Handle::current())?;
    session.set_yuv_output(true);
    let notify = std::sync::Arc::new(tokio::sync::Notify::new());
    let repaint = notify.clone();
    session.on_output(move || repaint.notify_one());
    let started = Instant::now();
    let mut count = 0;
    let mut decode = vec![];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    loop {
        tokio::select! { _ = notify.notified() => (), _ = tokio::time::sleep_until(deadline) => break }
        session.poll();
        if let Some(frame) = session.take_yuv_frame() {
            assert_eq!((frame.width, frame.height), (1920, 1080));
            assert_eq!(
                [
                    frame.planes[0].len(),
                    frame.planes[1].len(),
                    frame.planes[2].len()
                ],
                [1920 * 1080, 960 * 540, 960 * 540]
            );
            count += 1;
            decode.push(session.adb_video_stats().unwrap().last_decode_micros);
        }
        if let Status::Failed(error) = session.status() {
            anyhow::bail!(error);
        }
    }
    ensure!(count > 60, "I420 video stalled: {count}");
    ensure!(
        session.take_rgba_frame().is_none(),
        "GPU output unexpectedly retained RGBA"
    );
    decode.sort_unstable();
    eprintln!(
        "ADB native 1080p I420: received {:.2} fps (including startup); decode+packing p95 {:.3} ms; pixel bytes 3110400 instead of 8294400",
        count as f64 / started.elapsed().as_secs_f64(),
        decode[decode.len() * 95 / 100] as f64 / 1000.
    );
    drop(session);
    tokio::time::timeout(
        Duration::from_secs(8),
        tomato_services::remote_desktop::wait_adb_cleanup(),
    )
    .await?;
    ensure!(
        exe.with_extension("server-removed").exists(),
        "I420 actor did not remove its own server file"
    );
    Ok(())
}
