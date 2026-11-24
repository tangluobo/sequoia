//! Bounded compressed backlog. Never discard dependent H.264 frames and then decode their successors.
use super::*;
use h264_reader::nal::{Nal as _, RefNal};
use openh264::{decoder::Decoder, formats::YUVSource as _};
use std::{collections::VecDeque, sync::Condvar};
const MAX_PACKET: usize = 8 * 1024 * 1024;
const MAX_CONFIG: usize = 128 * 1024;
pub(super) fn dimensions(width: u32, height: u32) -> Result<()> {
    ensure!(
        width > 0
            && height > 0
            && width <= 8192
            && height <= 8192
            && u64::from(width) * u64::from(height) <= 8 * 1024 * 1024,
        "ADB 视频尺寸超过限制：{width}×{height}"
    );
    Ok(())
}
pub(super) struct Packet {
    pub pts: u64,
    pub data: Vec<u8>,
    pub key: bool,
    pub config: bool,
    pub reset: bool,
}
#[derive(Default)]
struct State {
    packets: VecDeque<Packet>,
    bytes: usize,
    config: Vec<u8>,
    config_pending: bool,
    awaiting_key: bool,
    closed: bool,
    resets: u64,
}
#[derive(Default)]
pub(super) struct Queue {
    state: Mutex<State>,
    ready: Condvar,
}
pub(super) struct Stop(pub Arc<Queue>);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.close();
    }
}
impl Queue {
    pub(super) fn offer(&self, mut packet: Packet) -> Result<()> {
        let mut s = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("ADB 视频队列锁失败"))?;
        if s.closed {
            return Ok(());
        }
        if packet.config {
            if !s.config_pending {
                s.config.clear();
            }
            ensure!(
                s.config.len() + packet.data.len() <= MAX_CONFIG,
                "H.264 参数集过大"
            );
            s.config.extend_from_slice(&packet.data);
            s.config_pending = true;
            return Ok(());
        }
        s.config_pending = false;
        let lag = s.packets.front().is_some_and(|first| {
            packet.pts.saturating_sub(first.pts) > 250_000 || packet.pts < first.pts
        });
        if lag || s.packets.len() >= 90 || s.bytes + packet.data.len() + s.config.len() > MAX_PACKET
        {
            s.packets.clear();
            s.bytes = 0;
            s.awaiting_key = true;
            s.resets += 1;
        }
        if s.awaiting_key && !packet.key {
            return Ok(());
        }
        packet.reset = s.awaiting_key;
        s.awaiting_key = false;
        if packet.key {
            let mut data = Vec::with_capacity(s.config.len() + packet.data.len());
            data.extend_from_slice(&s.config);
            data.extend_from_slice(&packet.data);
            packet.data = data;
        }
        s.bytes += packet.data.len();
        s.packets.push_back(packet);
        self.ready.notify_one();
        Ok(())
    }
    fn take(&self) -> Result<Option<(Packet, u64)>> {
        let mut s = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("ADB 视频队列锁失败"))?;
        while !s.closed && s.packets.is_empty() {
            s = self
                .ready
                .wait(s)
                .map_err(|_| anyhow::anyhow!("ADB 视频队列锁失败"))?;
        }
        let packet = s.packets.pop_front().map(|packet| {
            s.bytes -= packet.data.len();
            (packet, s.resets)
        });
        Ok(packet)
    }
    fn pending(&self) -> bool {
        self.state.lock().is_ok_and(|s| !s.packets.is_empty())
    }
    pub(super) fn close(&self) {
        if let Ok(mut s) = self.state.lock() {
            s.closed = true;
            s.packets.clear();
            s.config.clear();
            s.bytes = 0;
            self.ready.notify_all();
        }
    }
}
pub(super) async fn packet(reader: &mut (impl AsyncRead + Unpin)) -> Result<Packet> {
    let flags = reader.read_u64().await?;
    let size = reader.read_u32().await? as usize;
    ensure!(
        size > 0 && size <= MAX_PACKET,
        "无效的 H.264 包长度：{size}"
    );
    let mut data = vec![0; size];
    reader.read_exact(&mut data).await?;
    Ok(Packet {
        pts: flags & 0x3fff_ffff_ffff_ffff,
        data,
        config: flags & (1 << 63) != 0,
        key: flags & (1 << 62) != 0,
        reset: false,
    })
}
pub(super) async fn receive(mut socket: impl AsyncRead + Unpin, queue: Arc<Queue>) -> Result<()> {
    loop {
        // Android sends no video packets while its screen is unchanged. No idle timeout.
        queue.offer(packet(&mut socket).await?)?;
    }
}
fn validate_sps(bytes: &[u8]) -> Result<()> {
    for unit in openh264::nal_units(bytes) {
        let unit = if unit.starts_with(&[0, 0, 0, 1]) {
            &unit[4..]
        } else if unit.starts_with(&[0, 0, 1]) {
            &unit[3..]
        } else {
            unit
        };
        if unit.first().is_some_and(|b| b & 31 == 7) {
            ensure!(unit.len() <= MAX_CONFIG, "H.264 SPS 过大");
            let sps = h264_reader::nal::sps::SeqParameterSet::from_bits(
                RefNal::new(unit, &[], true).rbsp_bits(),
            )
            .map_err(|e| anyhow::anyhow!("无效的 H.264 SPS：{e:?}"))?;
            let (w, h) = sps
                .pixel_dimensions()
                .map_err(|e| anyhow::anyhow!("H.264 尺寸无效：{e:?}"))?;
            dimensions(w, h)?;
            ensure!(sps.max_num_ref_frames <= 16, "H.264 参考帧数量超过限制");
            ensure!(
                u64::from(w) * u64::from(h) * u64::from(sps.max_num_ref_frames.max(1))
                    <= 32 * 1024 * 1024,
                "H.264 参考画面内存超过限制"
            );
        }
    }
    Ok(())
}
fn packed_yuv(source: &impl openh264::formats::YUVSource) -> Result<YuvFrame> {
    let (width, height) = source.dimensions();
    dimensions(width as u32, height as u32)?;
    let stride = source.strides();
    let mut planes = [Vec::new(), Vec::new(), Vec::new()];
    for (index, (bytes, pitch, w, h)) in [
        (source.y(), stride.0, width, height),
        (source.u(), stride.1, width.div_ceil(2), height.div_ceil(2)),
        (source.v(), stride.2, width.div_ceil(2), height.div_ceil(2)),
    ]
    .into_iter()
    .enumerate()
    {
        ensure!(pitch >= w, "无效的 I420 行跨度");
        planes[index].reserve_exact(w * h);
        for row in 0..h {
            planes[index].extend_from_slice(
                bytes
                    .get(row * pitch..row * pitch + w)
                    .context("I420 平面数据不足")?,
            );
        }
    }
    Ok(YuvFrame {
        width: width as u16,
        height: height as u16,
        planes,
    })
}
pub(super) fn decode(
    queue: Arc<Queue>,
    shared: Arc<Mutex<Shared>>,
    notifier: Notifier,
) -> Result<()> {
    let generation = shared.lock().map(|s| s.adb_generation).unwrap_or_default();
    let mut decoder = Decoder::new()?;
    let mut sample = Instant::now();
    let mut count = 0u32;
    let mut fps = 0.;
    let mut decoded = 0;
    let mut published_at: Option<Instant> = None;
    while let Some((packet, resets)) = queue.take()? {
        validate_sps(&packet.data)?;
        if packet.reset {
            decoder = Decoder::new()?;
        }
        let started = Instant::now();
        let Some(yuv) = decoder.decode(&packet.data)? else {
            continue;
        };
        let (w, h) = yuv.dimensions();
        dimensions(w as u32, h as u32)?;
        count += 1;
        decoded += 1;
        if sample.elapsed() >= Duration::from_secs(1) {
            fps = count as f32 / sample.elapsed().as_secs_f32();
            count = 0;
            sample = Instant::now();
        }
        // Decode every reference. A busy queue must not indefinitely suppress every
        // completed picture; show the latest decoded frame while catching up.
        let visible = shared.lock().is_ok_and(|s| {
            s.status != Status::Ready
                || s.adb_frame_request_pending
                || s.adb_frame_requested_at
                    .is_none_or(|last| last.elapsed() < Duration::from_millis(250))
        });
        let defer = queue.pending()
            && published_at.is_some_and(|last| last.elapsed() < Duration::from_millis(8));
        if !visible || defer {
            continue;
        }
        let (use_rgba, use_yuv) = shared
            .lock()
            .map(|s| (s.adb_rgba_output, s.adb_yuv_output))
            .unwrap_or((false, false));
        let planar = if use_yuv {
            Some(packed_yuv(&yuv)?)
        } else {
            None
        };
        let rgba = if use_rgba {
            let mut pixels = vec![0u32; w * h];
            yuv.write_rgba8(bytemuck::cast_slice_mut(&mut pixels));
            Some(RgbaFrame {
                width: w as u16,
                height: h as u16,
                pixels,
            })
        } else {
            None
        };
        if let Ok(mut s) = shared.lock() {
            if s.adb_generation != generation {
                return Ok(());
            }
            let replaced = s.rgba_frame.is_some() || s.yuv_frame.is_some();
            s.rgba_frame = rgba;
            s.yuv_frame = planar;
            s.adb_frame_request_pending = false;
            let stats = s.adb_video.get_or_insert_with(VideoStats::default);
            stats.mode = "H.264";
            stats.size = [w as u16, h as u16];
            stats.frames_per_second = fps;
            stats.decoded_frames = decoded;
            stats.superseded_frames += u64::from(replaced);
            stats.queue_resets = resets;
            stats.last_decode_micros =
                started.elapsed().as_micros().min(u128::from(u64::MAX)) as u64;
            s.status = Status::Ready;
        }
        published_at = Some(Instant::now());
        wake(&notifier);
    }
    Ok(())
}
#[cfg(test)]
mod tests;
