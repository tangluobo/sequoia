//! Owned full-tile Progressive source for the actual Windows desktop codec path.
use super::*;

fn grid(width: u16, height: u16, level: i16, initial: bool) -> Vec<u8> {
    use ironrdp_graphics::progressive::{COEFFICIENTS_PER_COMPONENT, encode_first_pass};
    use ironrdp_pdu::codecs::rfx::{RfxRectangle, progressive::*};
    let quant = ComponentCodecQuant::LOSSLESS;
    let phase = usize::try_from((level + 48) / 6).unwrap();
    let encode = |value, text| {
        let mut component = [value; COEFFICIENTS_PER_COMPONENT];
        if text {
            for (index, sample) in component.iter_mut().enumerate() {
                let (x, y) = (index % 64, index / 64);
                if (3..14).contains(&(y % 18))
                    && (2..8).contains(&(x % 10))
                    && ((x / 10 + y / 18 + phase) ^ (x % 10 + y % 18)) & 3 == 0
                {
                    *sample = 77; // neutral-chroma text at luminance 205
                }
            }
        }
        let mut data = vec![0; 8192];
        let len = encode_first_pass(&mut component, &mut data, &quant, &quant, false).unwrap();
        data.truncate(len);
        data
    };
    let y = encode(level, true);
    let chroma = encode(0, false);
    let mut blocks = Vec::new();
    if initial {
        blocks.push(ProgressiveBlock::Sync(ProgressiveSyncPdu));
        blocks.push(ProgressiveBlock::Context(ProgressiveContextPdu {
            context_id: 0,
            tile_size: 64,
            flags: 0,
        }));
    }
    blocks.push(ProgressiveBlock::FrameBegin(ProgressiveFrameBeginPdu {
        frame_index: u32::try_from(phase).unwrap(),
        region_count: 1,
    }));
    let mut tiles = Vec::new();
    for ty in 0..height.div_ceil(64) {
        for tx in 0..width.div_ceil(64) {
            tiles.push(ProgressiveTile::Simple(TileSimple {
                quant_idx_y: 0,
                quant_idx_cb: 0,
                quant_idx_cr: 0,
                x_idx: tx,
                y_idx: ty,
                flags: 0,
                y_data: &y,
                cb_data: &chroma,
                cr_data: &chroma,
                tail_data: &[],
            }));
        }
    }
    blocks.push(ProgressiveBlock::Region(ProgressiveRegion {
        tile_size: 64,
        rects: vec![RfxRectangle {
            x: 0,
            y: 0,
            width,
            height,
        }],
        quant_vals: vec![quant],
        quant_prog_vals: vec![],
        flags: 0,
        tiles,
    }));
    blocks.push(ProgressiveBlock::FrameEnd(ProgressiveFrameEndPdu));
    encode_progressive_stream(&blocks).unwrap()
}

pub(super) async fn capture_fixture(motion: bool) -> Result<(Fixture, ServerTask)> {
    let graphics = GraphicsPeer::new();
    *graphics.capture_ready.lock().unwrap() = std::env::var_os("TOMATO_RDP_CAPTURE_PATH")
        .map(std::path::PathBuf::from)
        .map(|p| p.with_extension("ready"));
    let size = if motion {
        DesktopSize {
            width: 1920,
            height: 1080,
        }
    } else {
        DesktopSize {
            width: 320,
            height: 256,
        }
    };
    let fixture = Fixture::with_graphics(
        false,
        GraphicsDisplay(size),
        Some(Box::new(graphics.clone())),
    )
    .await?;
    let producer = ServerTask(tokio::task::spawn_local(async move {
        wait_until(|| Ok(graphics.server.lock().unwrap().is_ready())).await?;
        let surface = {
            let mut gfx = graphics.server.lock().unwrap();
            gfx.set_output_dimensions(size.width, size.height);
            let id = gfx
                .create_surface(size.width, size.height)
                .context("Progressive surface")?;
            anyhow::ensure!(gfx.map_surface_to_output(id, 0, 0));
            id
        };
        let frames = tokio::task::spawn_blocking(move || {
            (0..16)
                .map(|n| grid(size.width, size.height, n * 6 - 48, n == 0))
                .collect::<Vec<_>>()
        })
        .await?;
        eprintln!(
            "RDP Progressive source: frames_preencoded=16, tiles_per_frame={}, width={}, height={}",
            size.width.div_ceil(64) * size.height.div_ceil(64),
            size.width,
            size.height
        );
        let mut timing = SourceTiming::new("Progressive");
        let mut tick = tokio::time::interval(Duration::from_nanos(16_666_667));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut index = 0usize;
        loop {
            tick.tick().await;
            while graphics.server.lock().unwrap().should_backpressure() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            anyhow::ensure!(
                graphics
                    .server
                    .lock()
                    .unwrap()
                    .send_remotefx_progressive_frame(
                        surface,
                        1,
                        frames[index % 16].clone(),
                        (index as u32).wrapping_mul(16)
                    )
                    .is_some()
            );
            graphics.flush()?;
            timing.sent();
            index = index.wrapping_add(1);
            if !motion {
                std::future::pending::<()>().await;
            }
        }
    }));
    Ok((fixture, producer))
}

#[tokio::test]
async fn native_progressive_full_tile_frames_keep_geometry_and_change_pixels() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (fixture, _producer) = capture_fixture(true).await?;
            let mut session =
                DesktopSession::start(&fixture.connection, &tokio::runtime::Handle::current())?;
            let mut colors = std::collections::HashSet::new();
            let mut full = 0;
            wait_until(|| {
                session.poll();
                if let Status::Failed(message) = session.status() {
                    anyhow::bail!("{message}");
                }
                for update in session.take_rdp_updates() {
                    assert_eq!((update.frame.width, update.frame.height), (1920, 1080));
                    if update.region.is_none() {
                        full += 1;
                        let pixel = update.frame.pixels[1920 * 540 + 960];
                        let [r, g, b, a] = pixel.to_ne_bytes();
                        assert_eq!(a, 255);
                        assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1);
                        if r != 0 {
                            colors.insert(pixel);
                        }
                    }
                }
                Ok(full >= 5 && colors.len() >= 2)
            })
            .await?;
            eprintln!(
                "Progressive 1080p grid: full_updates={full}, distinct_center_colors={}",
                colors.len()
            );
            Ok(())
        })
        .await
}
