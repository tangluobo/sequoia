//! Real TLS/CredSSP/DRDYNVC/EGFX round trips against an owned loopback peer.
use super::*;
#[path = "rdpgfx_avc.rs"]
mod avc;
#[path = "rdpgfx_mosaic.rs"]
mod mosaic;
#[path = "rdpgfx_progressive.rs"]
mod progressive;
use ironrdp_egfx::{
    pdu::{CapabilitiesAdvertisePdu, CapabilitySet},
    server::{GraphicsPipelineHandler, GraphicsPipelineServer},
};
use ironrdp_server::{
    EgfxServerMessage, GfxDvcBridge, GfxServerFactory, GfxServerHandle, ServerEvent,
    ServerEventSender,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};

#[test]
fn clearcodec_matches_original_java_stateful_samples() {
    fn bytes(value: &serde_json::Value) -> Vec<u8> {
        let text = value.as_str().unwrap();
        (0..text.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
            .collect()
    }
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/rdpgfx-java-clearcodec.json")).unwrap();
    let mut decoder = ironrdp_graphics::clearcodec::ClearCodecDecoder::new();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let stream = bytes(&case["stream"]);
        let expected = bytes(&case["bgra"]);
        let actual = decoder.decode(
            &stream,
            u16::try_from(case["width"].as_u64().unwrap()).unwrap(),
            u16::try_from(case["height"].as_u64().unwrap()).unwrap(),
        );
        assert_eq!(
            actual.unwrap_or_else(|error| panic!("{name}: {error}")),
            expected,
            "{name}"
        );
    }
}

#[derive(Clone)]
struct GraphicsPeer {
    server: GfxServerHandle,
    sender: Arc<Mutex<Option<mpsc::UnboundedSender<ServerEvent>>>>,
    acknowledgements: Arc<AtomicU32>,
    capture_ready: Arc<Mutex<Option<std::path::PathBuf>>>,
}
struct GraphicsHandler(Arc<AtomicU32>, Arc<Mutex<Option<std::path::PathBuf>>>);
impl GraphicsPipelineHandler for GraphicsHandler {
    fn capabilities_advertise(&mut self, _: &CapabilitiesAdvertisePdu) {}
    fn on_ready(&mut self, _: &CapabilitySet) {}
    fn on_frame_ack(&mut self, _: u32, _: u32, _: u32) {
        self.0.fetch_add(1, Ordering::Relaxed);
        let path = self.1.lock().unwrap().take();
        if let Some(path) = path {
            use std::io::Write as _;
            std::fs::File::create_new(path)
                .and_then(|mut file| file.write_all(b"first_frame_ack\n"))
                .expect("Cannot publish owned fixture readiness");
        }
    }
}
impl GraphicsPeer {
    fn new() -> Self {
        let acknowledgements = Arc::new(AtomicU32::new(0));
        let capture_ready = Arc::new(Mutex::new(None));
        Self {
            server: Arc::new(Mutex::new(GraphicsPipelineServer::new(Box::new(
                GraphicsHandler(acknowledgements.clone(), capture_ready.clone()),
            )))),
            sender: Default::default(),
            acknowledgements,
            capture_ready,
        }
    }
    fn flush(&self) -> Result<()> {
        let messages = self.server.lock().unwrap().drain_output();
        self.send_messages(messages)
    }
    fn send_messages(&self, messages: Vec<ironrdp_dvc::DvcMessage>) -> Result<()> {
        let channel = self
            .server
            .lock()
            .unwrap()
            .channel_id()
            .context("EGFX channel not started")?;
        let messages = ironrdp_dvc::encode_dvc_messages(
            channel,
            messages,
            ironrdp_svc::ChannelFlags::SHOW_PROTOCOL,
        )?;
        self.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("Server event sender missing")?
            .send(ServerEvent::Egfx(EgfxServerMessage::SendMessages {
                messages,
            }))
            .map_err(|_| anyhow::anyhow!("Owned RDP server event loop closed"))?;
        Ok(())
    }
}
impl ServerEventSender for GraphicsPeer {
    fn set_sender(&mut self, sender: mpsc::UnboundedSender<ServerEvent>) {
        *self.sender.lock().unwrap() = Some(sender);
    }
}
impl GfxServerFactory for GraphicsPeer {
    fn build_gfx_handler(&self) -> Box<dyn GraphicsPipelineHandler> {
        Box::new(GraphicsHandler(
            self.acknowledgements.clone(),
            self.capture_ready.clone(),
        ))
    }
    fn build_server_with_handle(&self) -> Option<(GfxDvcBridge, GfxServerHandle)> {
        Some((GfxDvcBridge::new(self.server.clone()), self.server.clone()))
    }
}

struct GraphicsDisplay(DesktopSize);
#[async_trait::async_trait]
impl RdpServerDisplay for GraphicsDisplay {
    async fn size(&mut self) -> DesktopSize {
        self.0
    }
    async fn updates(&mut self) -> Result<Box<dyn RdpServerDisplayUpdates>> {
        Ok(Box::new(Updates(2))) // No legacy bitmap can accidentally satisfy this test.
    }
}

async fn wait_until(mut predicate: impl FnMut() -> Result<bool>) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(20), async {
        while !predicate()? {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("EGFX fixture timed out")?
}

#[derive(Default)]
struct SourceTiming {
    codec: &'static str,
    first: Option<std::time::Instant>,
    last: Option<std::time::Instant>,
    frames: usize,
    gaps: Vec<Duration>,
}
impl SourceTiming {
    fn new(codec: &'static str) -> Self {
        Self {
            codec,
            first: None,
            last: None,
            frames: 0,
            gaps: Vec::new(),
        }
    }
    fn sent(&mut self) {
        let now = std::time::Instant::now();
        if let Some(last) = self.last
            && self.gaps.len() < 4096
        {
            self.gaps.push(now.duration_since(last));
        }
        self.first.get_or_insert(now);
        self.last = Some(now);
        self.frames += 1;
    }
}
impl Drop for SourceTiming {
    fn drop(&mut self) {
        if let (Some(first), Some(last)) = (self.first, self.last) {
            self.gaps.sort_unstable();
            let p95 = self
                .gaps
                .get(self.gaps.len() * 95 / 100)
                .copied()
                .unwrap_or_default();
            eprintln!(
                "RDP EGFX source: frames={}, seconds={:.6}, max gap ms={:.3}; p95 gap ms={:.3}; source={} after ACK backpressure",
                self.frames,
                last.duration_since(first).as_secs_f64(),
                self.gaps.last().copied().unwrap_or_default().as_secs_f64() * 1000.,
                p95.as_secs_f64() * 1000.,
                self.codec,
            );
        }
    }
}

pub(super) async fn capture_fixture(motion: bool) -> Result<(Fixture, ServerTask)> {
    if std::env::var("TOMATO_RDP_CAPTURE_PATTERN").as_deref() == Ok("mosaic") {
        return mosaic::capture_fixture().await;
    }
    if std::env::var("TOMATO_RDP_CAPTURE_CODEC").as_deref() == Ok("progressive") {
        return progressive::capture_fixture(motion).await;
    }
    if matches!(
        std::env::var("TOMATO_RDP_CAPTURE_CODEC").as_deref(),
        Ok("adaptive" | "avc420")
    ) {
        return avc::capture_fixture(motion).await;
    }
    let graphics = GraphicsPeer::new();
    *graphics.capture_ready.lock().unwrap() = std::env::var_os("TOMATO_RDP_CAPTURE_PATH")
        .map(std::path::PathBuf::from)
        .map(|path| path.with_extension("ready"));
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
    let full = std::env::var("TOMATO_RDP_CAPTURE_PATTERN").as_deref() == Ok("full");
    let producer = ServerTask(tokio::task::spawn_local(async move {
        wait_until(|| Ok(graphics.server.lock().unwrap().is_ready())).await?;
        let surface = {
            let mut gfx = graphics.server.lock().unwrap();
            gfx.set_output_dimensions(size.width, size.height);
            let surface = gfx
                .create_surface(size.width, size.height)
                .context("Capture surface")?;
            anyhow::ensure!(
                gfx.map_surface_to_output(surface, 0, 0),
                "Capture output mapping"
            );
            surface
        };
        let mut tick = tokio::time::interval(Duration::from_nanos(16_666_667));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut index = 0_u32;
        let mut timing = SourceTiming::new("ClearCodec");
        loop {
            tick.tick().await;
            if graphics.server.lock().unwrap().should_backpressure() {
                continue;
            }
            let (left, top, width, height) = if index == 0 || full {
                (0, 0, size.width, size.height)
            } else {
                (100, 100, 32, 32)
            };
            let red = if index == 0 || !motion {
                12
            } else {
                20 + (index % 200) as u8
            };
            // Solid residual stream: the fixture avoids a full-frame encoder
            // allocation while the app receives real compressed DVC messages.
            let mut bytes = vec![0, index as u8];
            bytes.extend_from_slice(&10_u32.to_le_bytes());
            bytes.extend_from_slice(&[0; 8]);
            bytes.extend_from_slice(&[56, 34, red, 255, 255, 255]);
            bytes.extend_from_slice(&(u32::from(width) * u32::from(height)).to_le_bytes());
            {
                let mut gfx = graphics.server.lock().unwrap();
                anyhow::ensure!(
                    gfx.send_clearcodec_frame(
                        surface,
                        ironrdp_pdu::geometry::ExclusiveRectangle {
                            left,
                            top,
                            right: left + width,
                            bottom: top + height,
                        },
                        bytes,
                        index.wrapping_mul(16)
                    )
                    .is_some(),
                    "Capture frame rejected"
                );
            }
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
async fn native_rdpgfx_clearcodec_frames_acknowledgements_input_and_resize() -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            let graphics = GraphicsPeer::new();
            let Fixture {
                connection,
                server,
                mut received,
                _keys,
                ..
            } = Fixture::with_graphics(
                false,
                GraphicsDisplay(DesktopSize {
                    width: 320,
                    height: 256,
                }),
                Some(Box::new(graphics.clone())),
            )
            .await?;
            let mut session =
                DesktopSession::start(&connection, &tokio::runtime::Handle::current())?;
            wait_until(|| {
                session.poll();
                if let Status::Failed(message) = session.status() {
                    anyhow::bail!("{message}");
                }
                Ok(graphics.server.lock().unwrap().is_ready())
            })
            .await?;
            let surface = {
                let mut gfx = graphics.server.lock().unwrap();
                assert!(matches!(
                    gfx.negotiated_capabilities(),
                    Some(CapabilitySet::V10 { flags })
                        if *flags == ironrdp_egfx::pdu::CapabilitiesV10Flags::AVC_DISABLED
                ));
                assert!(!gfx.supports_avc420());
                gfx.set_output_dimensions(320, 256);
                let surface = gfx.create_surface(320, 256).context("Create surface")?;
                assert!(gfx.map_surface_to_output(surface, 0, 0));
                surface
            };
            // An idle login screen may send a Progressive tile but defer EndFrame.
            // It must become visible and accept input without waiting for that PDU.
            let (initial, delayed_end) = {
                let mut gfx = graphics.server.lock().unwrap();
                assert!(
                    gfx.send_remotefx_progressive_frame(
                        surface,
                        7,
                        progressive_tile_stream(0, 0, 64, 64),
                        0
                    )
                    .is_some()
                );
                let mut messages = gfx.drain_output();
                let end = messages.pop().context("Missing delayed EndFrame")?;
                (messages, end)
            };
            graphics.send_messages(initial)?;
            wait_until(|| {
                session.poll();
                if let Status::Failed(message) = session.status() {
                    anyhow::bail!("{message}");
                }
                Ok(session.take_frame().is_some_and(|image| {
                    image.width == 320
                        && image.height == 256
                        && image.pixels[0] == 0x808080
                        && image.pixels[64] == 0
                }))
            })
            .await?;
            assert_eq!(graphics.acknowledgements.load(Ordering::Relaxed), 0);
            graphics.send_messages(vec![delayed_end])?;
            wait_until(|| Ok(graphics.acknowledgements.load(Ordering::Relaxed) == 1)).await?;
            let mut encoder = ironrdp_graphics::clearcodec::ClearCodecEncoder::new();
            for frame in 0..8_u8 {
                let color = [56, 34, 12 + frame, 255];
                let encoded = encoder.encode(&color.repeat(320 * 256), 320, 256);
                {
                    let mut gfx = graphics.server.lock().unwrap();
                    assert!(
                        gfx.send_clearcodec_frame(
                            surface,
                            ironrdp_pdu::geometry::ExclusiveRectangle {
                                left: 0,
                                top: 0,
                                right: 320,
                                bottom: 256,
                            },
                            encoded,
                            u32::from(frame) * 16
                        )
                        .is_some()
                    );
                }
                graphics.flush()?;
                wait_until(|| {
                    session.poll();
                    if let Status::Failed(message) = session.status() {
                        anyhow::bail!("{message}");
                    }
                    Ok(session.take_frame().is_some_and(|image| {
                        image.width == 320
                            && image.height == 256
                            && image
                                .pixels
                                .iter()
                                .all(|&p| p == (u32::from(12 + frame) << 16 | 0x2238))
                    }))
                })
                .await?;
            }
            wait_until(|| Ok(graphics.acknowledgements.load(Ordering::Relaxed) >= 9)).await?;
            session.send(Input::Pointer {
                x: 71,
                y: 82,
                button: None,
                down: false,
                wheel: 0,
            })?;
            tokio::time::timeout(Duration::from_secs(2), async {
                while !received
                    .recv()
                    .await
                    .context("Input peer closed")?
                    .contains("71")
                {}
                Ok::<_, anyhow::Error>(())
            })
            .await??;
            {
                let mut gfx = graphics.server.lock().unwrap();
                gfx.resize(384, 288);
                let new_surface = gfx.create_surface(384, 288).context("Resize surface")?;
                assert!(gfx.map_surface_to_output(new_surface, 0, 0));
                assert!(
                    gfx.send_uncompressed_frame(
                        new_surface,
                        &[80, 60, 40, 255].repeat(384 * 288),
                        384,
                        288,
                        160
                    )
                    .is_some()
                );
            }
            graphics.flush()?;
            wait_until(|| {
                session.poll();
                if let Status::Failed(message) = session.status() {
                    anyhow::bail!("{message}");
                }
                Ok(session.take_frame().is_some_and(|image| {
                    image.width == 384
                        && image.height == 288
                        && image.pixels.iter().all(|&p| p == 0x283c50)
                }))
            })
            .await?;
            drop(session);
            drop(server);
            Ok(())
        })
        .await
}

fn progressive_tile_stream(tile_x: u16, tile_y: u16, rect_width: u16, rect_height: u16) -> Vec<u8> {
    use ironrdp_graphics::progressive::{COEFFICIENTS_PER_COMPONENT, encode_first_pass};
    use ironrdp_pdu::codecs::rfx::RfxRectangle;
    use ironrdp_pdu::codecs::rfx::progressive::{
        ComponentCodecQuant, ProgressiveBlock, ProgressiveContextPdu, ProgressiveFrameBeginPdu,
        ProgressiveFrameEndPdu, ProgressiveRegion, ProgressiveSyncPdu, ProgressiveTile, TileSimple,
        encode_progressive_stream,
    };

    let base_quant = ComponentCodecQuant::LOSSLESS;
    let mut component = [0i16; COEFFICIENTS_PER_COMPONENT];
    let mut component_data = [0u8; 8192];
    let component_len = encode_first_pass(
        &mut component,
        &mut component_data,
        &base_quant,
        &ComponentCodecQuant::LOSSLESS,
        false,
    )
    .unwrap();
    let component_data = &component_data[..component_len];

    encode_progressive_stream(&[
        ProgressiveBlock::Sync(ProgressiveSyncPdu),
        ProgressiveBlock::Context(ProgressiveContextPdu {
            context_id: 0,
            tile_size: 0x0040,
            flags: 0,
        }),
        ProgressiveBlock::FrameBegin(ProgressiveFrameBeginPdu {
            frame_index: 0,
            region_count: 1,
        }),
        ProgressiveBlock::Region(ProgressiveRegion {
            tile_size: 0x40,
            rects: vec![RfxRectangle {
                x: tile_x.saturating_mul(64),
                y: tile_y.saturating_mul(64),
                width: rect_width,
                height: rect_height,
            }],
            quant_vals: vec![base_quant],
            quant_prog_vals: vec![],
            flags: 0,
            tiles: vec![ProgressiveTile::Simple(TileSimple {
                quant_idx_y: 0,
                quant_idx_cb: 0,
                quant_idx_cr: 0,
                x_idx: tile_x,
                y_idx: tile_y,
                flags: 0,
                y_data: component_data,
                cb_data: component_data,
                cr_data: component_data,
                tail_data: &[],
            })],
        }),
        ProgressiveBlock::FrameEnd(ProgressiveFrameEndPdu),
    ])
    .unwrap()
}
