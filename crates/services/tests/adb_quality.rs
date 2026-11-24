//! Manual high-resolution stress test, separate from the standard scrcpy CI suite.
#[path = "support/adb_peer.rs"]
mod adb_peer;
use adb_peer::Fixture;
use anyhow::{Context, Result, ensure};
use std::time::{Duration, Instant};
use tomato_services::remote_desktop::{DesktopSession, Status};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned adb_fixture and TOMATO_TEST_ADB_VIDEO; steady high-resolution video"]
async fn high_resolution_owned_video_keeps_publishing_without_a_gui() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let exe = Fixture::start(temp.path())?;
    std::fs::copy(
        std::env::var_os("TOMATO_TEST_ADB_VIDEO").context("Missing owned video fixture")?,
        exe.with_extension("video.scrcpy"),
    )?;
    let mut session = DesktopSession::start(&exe.connection(), &tokio::runtime::Handle::current())?;
    session.set_yuv_output(true);
    let started = Instant::now();
    let mut first = None;
    let mut frames = 0;
    let mut size = [0; 2];
    loop {
        session.poll();
        if let Some(frame) = session.take_yuv_frame() {
            first.get_or_insert_with(Instant::now);
            size = [frame.width, frame.height];
            frames += 1;
        }
        if let Status::Failed(error) = session.status() {
            anyhow::bail!(error);
        }
        if first.is_some_and(|first| first.elapsed() >= Duration::from_secs(4)) {
            break;
        }
        ensure!(
            started.elapsed() < Duration::from_secs(20),
            "Video never published"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    let stats = session
        .adb_video_stats()
        .context("Missing video statistics")?;
    eprintln!(
        "High-resolution owned stream: {size:?}, {frames} frames over {:.3}s after first frame; decoder FPS {:.2}, decoded {}, queue resets {}, last decode {}us",
        first.unwrap().elapsed().as_secs_f64(),
        stats.frames_per_second,
        stats.decoded_frames,
        stats.queue_resets,
        stats.last_decode_micros
    );
    ensure!(frames > 60, "High-resolution stream stalled: {frames}");
    drop(session);
    tokio::time::timeout(
        Duration::from_secs(8),
        tomato_services::remote_desktop::wait_adb_cleanup(),
    )
    .await?;
    Ok(())
}
