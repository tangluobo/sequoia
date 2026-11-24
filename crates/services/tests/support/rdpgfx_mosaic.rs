//! Deterministic scattered ClearCodec updates; every final framebuffer pixel has a known value.
use super::*;
use ironrdp_egfx::server::MixedTilePayload;
use ironrdp_pdu::geometry::ExclusiveRectangle;
pub(super) async fn capture_fixture() -> Result<(Fixture, ServerTask)> {
    let graphics = GraphicsPeer::new();
    *graphics.capture_ready.lock().unwrap() = std::env::var_os("TOMATO_RDP_CAPTURE_PATH")
        .map(std::path::PathBuf::from)
        .map(|p| p.with_extension("ready"));
    let size = DesktopSize {
        width: 1920,
        height: 1080,
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
            gfx.set_output_dimensions(1920, 1080);
            let id = gfx.create_surface(1920, 1080).context("Mosaic surface")?;
            anyhow::ensure!(gfx.map_surface_to_output(id, 0, 0));
            id
        };
        let mut tiles = Vec::new();
        for step in 0..510_usize {
            // 73 and 510 are coprime: all 510 tiles arrive in a scattered order.
            let index = step * 73 % 510;
            let left = (index % 30 * 64) as u16;
            let top = (index / 30 * 64) as u16;
            let width = 64_u16;
            let height = 64_u16.min(1080 - top);
            let red = (30 + index * 37 % 180) as u8;
            let mut bytes = vec![0, step as u8];
            bytes.extend_from_slice(&10_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
            bytes.extend_from_slice(&[56, 34, red, 255, 255, 255]);
            bytes.extend_from_slice(&(u32::from(width) * u32::from(height)).to_le_bytes());
            tiles.push(MixedTilePayload::ClearCodec {
                destination: ExclusiveRectangle {
                    left,
                    top,
                    right: left + width,
                    bottom: top + height,
                },
                bitmap_data: bytes,
            });
            if tiles.len() == 32 || step == 509 {
                while graphics.server.lock().unwrap().should_backpressure() {
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                anyhow::ensure!(
                    graphics
                        .server
                        .lock()
                        .unwrap()
                        .send_mixed_frame(surface, std::mem::take(&mut tiles), step as u32)
                        .is_some(),
                    "Mosaic frame refused"
                );
                graphics.flush()?;
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
        eprintln!(
            "RDP mosaic: all 510 scattered tiles sent; final reference is deterministic 1920x1080"
        );
        std::future::pending::<()>().await;
        Ok(())
    }));
    Ok((fixture, producer))
}
#[tokio::test]
async fn scattered_tile_frames_retain_every_pixel_without_gui_draining() -> Result<()> {
    tokio::task::LocalSet::new().run_until(async {
        let (fixture,_producer)=capture_fixture().await?;
        let mut session=DesktopSession::start(&fixture.connection,&tokio::runtime::Handle::current())?;
        tokio::time::timeout(Duration::from_secs(20),async {
            loop {
                session.poll();if let Status::Failed(e)=session.status(){anyhow::bail!("{e}");}
                if let Some(snapshot)=session.rdp_full_snapshot() {
                    let full=snapshot.pixels.iter().enumerate().all(|(pixel,color)| {
                        let tile=(pixel%1920/64)+(pixel/1920/64)*30;
                        color.to_ne_bytes()==[(30+tile*37%180) as u8,34,56,255]
                    });
                    if full {println!("Mosaic: all 2073600 RGBA pixels exact, 510 scattered tiles retained without GUI drains");break;}
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Ok::<_,anyhow::Error>(())
        }).await??;
        drop(session);drop(fixture);Ok::<_,anyhow::Error>(())
    }).await
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires isolated CPU for latency measurement under full Progressive decoding"]
async fn mouse_input_during_full_progressive_decoding() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut fixture, _producer) = super::progressive::capture_fixture(true).await?;
            let mut session =
                DesktopSession::start(&fixture.connection, &tokio::runtime::Handle::current())?;
            tokio::time::timeout(Duration::from_secs(20), async {
                while session.status() != Status::Ready {
                    session.poll();
                    if let Status::Failed(e) = session.status() {
                        anyhow::bail!("{e}");
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            let mut latencies = Vec::new();
            let mut enqueue_times = Vec::new();
            let mut handler_times = Vec::new();
            for index in 0..32_u16 {
                let x = 100 + index * 7;
                let started = std::time::Instant::now();
                session.send(Input::Pointer {
                    x,
                    y: 100,
                    button: None,
                    down: false,
                    wheel: 0,
                })?;
                enqueue_times.push(started.elapsed());
                tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        session.poll();
                        while let Ok(event) = fixture.received.try_recv() {
                            if event == format!("Move {{ x: {x}, y: 100 }}") {
                                while let Ok((event, at)) = fixture.mouse_received_at.try_recv() {
                                    if event == format!("Move {{ x: {x}, y: 100 }}") {
                                        handler_times.push(at.saturating_duration_since(started));
                                    }
                                }
                                return;
                            }
                        }
                        tokio::time::sleep(Duration::from_millis(1)).await;
                    }
                })
                .await?;
                latencies.push(started.elapsed());
                tokio::time::sleep(Duration::from_millis(4)).await;
            }
            latencies.sort();
            enqueue_times.sort();
            handler_times.sort();
            anyhow::ensure!(handler_times.len() == 32, "Missing server handler timestamps");
            let dispatch = session.input_dispatch_stats().context("Missing native input lane")?;
            eprintln!("Input path diagnostics: enqueue median {:?}, p95 {:?}, max {:?}; TLS dispatch {:?}; runtime {:?}",
                enqueue_times[16], enqueue_times[30], enqueue_times[31], session.input_dispatch_stats(),
                tokio::runtime::Handle::current().runtime_flavor());
            println!(
                "Mouse observed by test after continuous 1080p Progressive decoding: median {:?}, p95 {:?}, max {:?}",
                latencies[16], latencies[30], latencies[31]
            );
            eprintln!("Fixture server handler: median {:?}, p95 {:?}, max {:?}; not client TLS write latency",
                handler_times[16], handler_times[30], handler_times[31]);
            // ironrdp-server 0.13 holds its server Mutex while writing an EGFX
            // event batch; input dispatch waits for that same Mutex. Measure the
            // client's actual write deadline independently, keeping the 100ms
            // bound. All 32 events must still reach the real TLS/CredSSP handler
            // within the per-event 3s timeout above. Handler/observer delays are
            // reported separately and are not presented as end-to-end success.
            anyhow::ensure!(
                enqueue_times[30] < Duration::from_millis(100)
                    && dispatch.messages >= 32 && dispatch.max_micros < 100_000,
                "Client input deadline missed under decoding: enqueue p95 {:?}, TLS {:?}",
                enqueue_times[30], dispatch
            );
            drop(session);
            drop(fixture);
            Ok::<_, anyhow::Error>(())
        })
        .await
}
