use super::*;
use std::time::Instant;

pub async fn control(files: PathBuf, mut stream: DuplexStream) -> Result<()> {
    let log = files.with_extension("scrcpy-input.jsonl");
    while let Ok(kind) = stream.read_u8().await {
        let size = match kind {
            0 => 14,
            1 => 5,
            2 => 32,
            3 => 21,
            _ => anyhow::bail!("Unexpected scrcpy control byte"),
        };
        let mut bytes = vec![0; size];
        bytes[0] = kind;
        stream.read_exact(&mut bytes[1..]).await?;
        if kind == 1 {
            let len = u32::from_be_bytes(bytes[1..5].try_into()?) as usize;
            ensure!(len <= 300, "Unbounded text");
            bytes.resize(5 + len, 0);
            stream.read_exact(&mut bytes[5..]).await?;
        }
        let mut line = serde_json::to_vec(&bytes)?;
        line.push(b'\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)?
            .write_all(&line)?;
    }
    Ok(())
}

pub async fn video(files: PathBuf, mut stream: DuplexStream) -> Result<()> {
    let source = std::fs::read(files.with_extension("video.scrcpy")).unwrap_or_else(|_| {
        let mut source = b"h264".to_vec();
        source.extend(64u32.to_be_bytes());
        source.extend(48u32.to_be_bytes());
        source.extend_from_slice(include_bytes!("../../tests/fixtures/adb/red64x48.scrcpy"));
        source
    });
    stream.write_u8(0).await?;
    stream.write_all(&source[..12]).await?;
    let mut packets = Vec::new();
    let mut offset = 12;
    while offset < source.len() {
        let flags = u64::from_be_bytes(source[offset..offset + 8].try_into()?);
        let size = u32::from_be_bytes(source[offset + 8..offset + 12].try_into()?) as usize;
        packets.push((flags, &source[offset + 8..offset + 12 + size]));
        offset += 12 + size;
    }
    let duration = packets
        .iter()
        .rev()
        .find_map(|(flags, _)| (flags & (1 << 63) == 0).then_some(flags & 0x3fff_ffff_ffff_ffff))
        .context("ADB fixture has no video frames")?
        + 16_667;

    let started = Instant::now();
    let mut frames = 0u64;
    let mut last_frame = None;
    let mut max_gap = 0.;
    let mut bursts = 0;
    for cycle in (0u64..).map(|n| n * duration) {
        for &(flags, packet) in &packets {
            let is_frame = flags & (1 << 63) == 0;
            let pts = cycle + (flags & 0x3fff_ffff_ffff_ffff);
            if is_frame {
                tokio::time::sleep_until((started + Duration::from_micros(pts)).into()).await;
            }
            stream
                .write_u64((flags & 0xc000_0000_0000_0000) | pts)
                .await?;
            stream.write_all(packet).await?;
            if is_frame {
                frames += 1;
                let now = Instant::now();
                if let Some(last) = last_frame {
                    let gap = now.duration_since(last).as_secs_f64() * 1000.;
                    max_gap = f64::max(max_gap, gap);
                    bursts += u64::from(gap < 2.);
                }
                last_frame = Some(now);
                if frames == 1 || frames.is_multiple_of(6) {
                    let elapsed = started.elapsed().as_secs_f64();
                    let summary = serde_json::json!({
                        "emittedFrames": frames,
                        "durationSeconds": elapsed,
                        "framesPerSecond": frames as f64 / elapsed.max(0.001),
                        "maxGapMs": max_gap,
                        "burstFrames": bursts
                    });
                    for (extension, bytes) in [
                        ("h264-heartbeat", frames.to_string().into_bytes()),
                        ("video-stats.json", serde_json::to_vec(&summary)?),
                    ] {
                        let temporary = files.with_extension(format!("{extension}.tmp"));
                        std::fs::write(&temporary, bytes)?;
                        std::fs::rename(&temporary, files.with_extension(extension))?;
                    }
                }
            }
        }
    }
    Ok(())
}
