//! Real negotiated H.264, inter-frame state, dirty rectangles and lossless fallback.
use super::*;
use ironrdp_egfx::pdu::Avc420Region;
use openh264::{
    OpenH264API,
    encoder::{BitRate, Encoder, EncoderConfig},
    formats::YUVBuffer,
};

#[tokio::test]
async fn native_avc420_decodes_inter_frames_regions_and_mixed_lossless_updates() -> Result<()> {
    tokio::task::LocalSet::new().run_until(async {
        let graphics = GraphicsPeer::new();
        let Fixture { mut connection, server, .. } = Fixture::with_graphics(false,
            GraphicsDisplay(DesktopSize {width:320,height:256}),Some(Box::new(graphics.clone()))).await?;
        connection.set("rdpAvc420", true);
        let mut session=DesktopSession::start(&connection,&tokio::runtime::Handle::current())?;
        wait_until(|| {
            session.poll();
            if let Status::Failed(message)=session.status(){anyhow::bail!("{message}");}
            Ok(graphics.server.lock().unwrap().is_ready())
        }).await?;
        let surface={
            let mut gfx=graphics.server.lock().unwrap();
            assert!(matches!(gfx.negotiated_capabilities(),Some(CapabilitySet::V8_1{..})));
            assert!(gfx.supports_avc420());
            assert!(!gfx.supports_avc444());
            gfx.set_output_dimensions(320,256);
            let surface=gfx.create_surface(320,256).context("AVC surface")?;
            assert!(gfx.map_surface_to_output(surface,0,0));
            surface
        };
        let mut encoder=Encoder::with_api_config(OpenH264API::from_source(),
            EncoderConfig::new().bitrate(BitRate::from_bps(8_000_000)).skip_frames(false))?;
        let mut canvas=vec![0u32;320*256];
        let mut compressed=0usize;
        let start=std::time::Instant::now();
        for n in 0..12u8 {
            let value=48+n*8;
            let mut yuv=vec![128;320*256*3/2];
            yuv[..320*256].fill(value);
            let mut encoded=encoder.encode(&YUVBuffer::from_vec(yuv,320,256))?.to_vec();
            // The first packet starts with SPS/PPS: its legacy length prefixes
            // are unambiguous. Later slice lengths can begin 00 00 01 xx,
            // indistinguishable from the required Annex B start code.
            if n==0 {
                let mut avc=Vec::new();
                for nal in openh264::nal_units(&encoded) {
                    let prefix=if nal.starts_with(&[0,0,0,1]){4}else{3};
                    let body=&nal[prefix..];
                    avc.extend_from_slice(&(body.len() as u32).to_be_bytes());
                    avc.extend_from_slice(body);
                }
                encoded=avc;
            }
            compressed+=encoded.len();
            let region=if n==0 {Avc420Region::full_frame(320,256,0)} else {Avc420Region::new(96,64,160,128,0,100)};
            assert!(graphics.server.lock().unwrap().send_avc420_frame(surface,&encoded,&[region],u32::from(n)*16).is_some());
            graphics.flush()?;
            wait_until(|| {
                session.poll();
                if let Status::Failed(message)=session.status(){anyhow::bail!("{message}");}
                let updates=session.take_rdp_updates();
                if updates.is_empty(){return Ok(false);}
                for update in updates {
                    let r=update.region.unwrap_or(tomato_services::remote_desktop::Region{x:0,y:0,width:320,height:256});
                    for (row,pixels) in update.frame.pixels.chunks_exact(usize::from(r.width)).enumerate(){
                        let offset=(usize::from(r.y)+row)*320+usize::from(r.x);
                        canvas[offset..offset+pixels.len()].copy_from_slice(pixels);
                    }
                }
                let [r,g,b,a]=canvas[80*320+120].to_ne_bytes();
                Ok(r.abs_diff(value)<=2 && g.abs_diff(value)<=2 && b.abs_diff(value)<=2 && a==255)
            }).await.with_context(||format!("AVC frame {n}"))?;
            let [r,g,b,a]=canvas[0].to_ne_bytes();
            assert!(r.abs_diff(48)<=2 && g.abs_diff(48)<=2 && b.abs_diff(48)<=2 && a==255,
                "AVC dirty regions must not overwrite the unchanged background");
        }
        let mut yuv=vec![100;320*256*3/2];
        yuv[320*256..320*256*5/4].fill(90);
        yuv[320*256*5/4..].fill(200);
        let encoded=encoder.encode(&YUVBuffer::from_vec(yuv,320,256))?.to_vec();
        assert!(graphics.server.lock().unwrap().send_avc420_frame(surface,&encoded,&[Avc420Region::full_frame(320,256,0)],192).is_some());
        graphics.flush()?;
        wait_until(|| {
            session.poll();
            if let Status::Failed(message)=session.status(){anyhow::bail!("{message}");}
            let updates=session.take_rdp_updates();
            if let Some(u)=updates.first(){eprintln!("BT709 synthetic sample: {:?}",u.frame.pixels[0].to_ne_bytes());}
            Ok(updates.iter().any(|u|u.frame.pixels.iter().any(|p|{
                let [r,g,b,a]=p.to_ne_bytes();
                r.abs_diff(213)<=3 && g.abs_diff(73)<=3 && b.abs_diff(29)<=3 && a==255
            })))
        }).await.context("BT709 reference frame")?;
        let mut encoder=ironrdp_graphics::clearcodec::ClearCodecEncoder::new();
        let stream=encoder.encode(&[21,43,65,255].repeat(16*16),16,16);
        assert!(graphics.server.lock().unwrap().send_clearcodec_frame(surface,
            ironrdp_pdu::geometry::ExclusiveRectangle{left:4,top:4,right:20,bottom:20},stream,200).is_some());
        graphics.flush()?;
        wait_until(|| {
            session.poll();
            if let Status::Failed(message)=session.status(){anyhow::bail!("{message}");}
            Ok(session.take_rdp_updates().iter().any(|u| u.frame.pixels.iter().any(|p|p.to_ne_bytes()==[65,43,21,255])))
        }).await?;
        wait_until(|| Ok(graphics.acknowledgements.load(Ordering::Relaxed)>=14)).await?;
        println!("AVC420 loopback: frames=13, interframe_sequence_bytes={compressed}, elapsed_ms={}, ROI_background_preserved=true, BT709_full_range=true, ClearCodec_fallback=true",start.elapsed().as_millis());
        drop(session);drop(server);
        Ok(())
    }).await
}

enum Encoded {
    Avc(Vec<u8>, Vec<Avc420Region>),
    Clear(Vec<(ironrdp_pdu::geometry::ExclusiveRectangle, Vec<u8>)>),
}

// Encode ahead of measurement: source encoding and full-screen buffer allocation
// must not cap the GUI's measured submission rate. The first frame of every
// repeated 16-frame GOP is an IDR, so reference frames remain valid on wraparound.
pub(super) async fn capture_fixture(motion: bool) -> Result<(Fixture, ServerTask)> {
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
        let (surface, avc) = {
            let mut gfx = graphics.server.lock().unwrap();
            gfx.set_output_dimensions(size.width, size.height);
            let surface = gfx
                .create_surface(size.width, size.height)
                .context("Adaptive surface")?;
            anyhow::ensure!(gfx.map_surface_to_output(surface, 0, 0));
            (surface, gfx.supports_avc420())
        };
        let frames = tokio::task::spawn_blocking(move || -> Result<Vec<Encoded>> {
            let width = usize::from(size.width);
            let height = usize::from(size.height);
            let mut avc_encoder = if avc {
                Some(Encoder::with_api_config(
                    OpenH264API::from_source(),
                    EncoderConfig::new()
                        .bitrate(BitRate::from_bps(16_000_000))
                        .skip_frames(false),
                )?)
            } else {
                None
            };
            let mut clear_encoder = ironrdp_graphics::clearcodec::ClearCodecEncoder::new();
            let mut frames = Vec::new();
            let mut held = 0usize;
            for n in 0..16u8 {
                let value = 48 + n * 6;
                let regions = if full || n == 0 {
                    vec![Avc420Region::full_frame(size.width, size.height, 0)]
                } else {
                    vec![
                        Avc420Region::new(0, 0, 32, 32, 0, 100),
                        Avc420Region::new(
                            size.width - 32,
                            size.height - 32,
                            size.width,
                            size.height,
                            0,
                            100,
                        ),
                    ]
                };
                let mut luma = vec![48; width * height];
                for r in &regions {
                    for y in r.top..r.bottom {
                        luma[usize::from(y) * width + usize::from(r.left)
                            ..usize::from(y) * width + usize::from(r.right)]
                            .fill(value);
                    }
                }
                if motion && full {
                    // A structured text-like scene, not a single RLE color run.
                    // Both codecs receive these exact same luminance samples.
                    for y in 0..height {
                        for x in 0..width {
                            if (3..14).contains(&(y % 18))
                                && (2..8).contains(&(x % 10))
                                && ((x / 10 + y / 18 + usize::from(n)) ^ (x % 10 + y % 18)) & 3 == 0
                            {
                                luma[y * width + x] = 205;
                            }
                        }
                    }
                }
                if let Some(encoder) = &mut avc_encoder {
                    let mut pixels = luma;
                    pixels.resize(width * height * 3 / 2, 128);
                    let encoded = encoder
                        .encode(&YUVBuffer::from_vec(pixels, width, height))?
                        .to_vec();
                    held += encoded.len();
                    frames.push(Encoded::Avc(encoded, regions));
                } else {
                    let mut patches = Vec::new();
                    for r in regions {
                        let w = r.right - r.left;
                        let h = r.bottom - r.top;
                        let mut bgra = Vec::with_capacity(usize::from(w) * usize::from(h) * 4);
                        for y in r.top..r.bottom {
                            for x in r.left..r.right {
                                let gray = luma[usize::from(y) * width + usize::from(x)];
                                bgra.extend_from_slice(&[gray, gray, gray, 255]);
                            }
                        }
                        let data = clear_encoder.encode(&bgra, w, h);
                        held += data.len();
                        patches.push((r.to_rectangle(), data));
                    }
                    frames.push(Encoded::Clear(patches));
                }
                anyhow::ensure!(
                    held <= 64 * 1024 * 1024,
                    "Owned source exceeded bounded GOP budget"
                );
            }
            Ok(frames)
        })
        .await??;
        let codec = if avc { "AVC420" } else { "ClearCodec" };
        eprintln!(
            "RDP adaptive source: codec={codec}, frames_preencoded=16, width={}, height={}",
            size.width, size.height
        );
        let mut timing = SourceTiming::new(codec);
        let mut tick = tokio::time::interval(Duration::from_nanos(16_666_667));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut index = 0usize;
        let mut clear_sequence = 0u8;
        loop {
            tick.tick().await;
            while graphics.server.lock().unwrap().should_backpressure() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            {
                let mut gfx = graphics.server.lock().unwrap();
                match &frames[index % frames.len()] {
                    Encoded::Avc(data, regions) => {
                        anyhow::ensure!(
                            gfx.send_avc420_frame(
                                surface,
                                data,
                                regions,
                                (index as u32).wrapping_mul(16)
                            )
                            .is_some()
                        );
                    }
                    Encoded::Clear(patches) => {
                        for (rect, data) in patches {
                            let mut data = data.clone();
                            data[1] = clear_sequence;
                            clear_sequence = clear_sequence.wrapping_add(1);
                            anyhow::ensure!(
                                gfx.send_clearcodec_frame(
                                    surface,
                                    rect.clone(),
                                    data,
                                    (index as u32).wrapping_mul(16)
                                )
                                .is_some()
                            );
                        }
                    }
                }
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
