//! Optional aggregate timings: no credentials, compressed bytes or pixels.
use std::time::{Duration, Instant};

pub(crate) struct Timing {
    since: Instant,
    decompress: Duration,
    parse: Duration,
    handle: Duration,
    messages: u64,
    progressive: u64,
    surface1: u64,
    frames: u64,
    commands: [Duration; 8],
    command_counts: [u64; 8],
    last_frame: Option<(Instant, Option<u64>)>,
    local_gaps: [u64; 5],
    server_gaps: [u64; 5],
    missing_timestamps: u64,
    records: std::collections::VecDeque<String>,
}

impl Timing {
    pub(crate) fn enabled() -> Option<Self> {
        (std::env::var("TOMATO_RDP_PIPELINE_TIMING").as_deref() == Ok("1")).then(|| Self {
            since: Instant::now(),
            decompress: Duration::ZERO,
            parse: Duration::ZERO,
            handle: Duration::ZERO,
            messages: 0,
            progressive: 0,
            surface1: 0,
            frames: 0,
            commands: [Duration::ZERO; 8],
            command_counts: [0; 8],
            last_frame: None,
            local_gaps: [0; 5],
            server_gaps: [0; 5],
            missing_timestamps: 0,
            records: std::collections::VecDeque::new(),
        })
    }
    pub(crate) fn pdu(&mut self, pdu: &crate::pdu::GfxPdu) {
        use crate::pdu::GfxPdu;
        match pdu {
            GfxPdu::WireToSurface1(_) => self.surface1 += 1,
            GfxPdu::WireToSurface2(_) => self.progressive += 1,
            GfxPdu::EndFrame(_) => self.frames += 1,
            GfxPdu::StartFrame(start) => {
                let stamp = &start.timestamp;
                // MS-RDPEGFX 2.2.2.11: zero means unavailable; valid UTC hours
                // wrap at midnight. Absent stamps do not measure server pacing.
                let valid = stamp.hours < 24
                    && stamp.minutes < 60
                    && stamp.seconds < 60
                    && stamp.milliseconds < 1000;
                let millis = (((u64::from(stamp.hours) * 60 + u64::from(stamp.minutes)) * 60
                    + u64::from(stamp.seconds))
                    * 1000)
                    + u64::from(stamp.milliseconds);
                let current = (valid && millis != 0).then_some(millis);
                if current.is_none() {
                    self.missing_timestamps += 1;
                }
                let now = Instant::now();
                if let Some((previous, server)) = self.last_frame {
                    let local =
                        u64::try_from(now.duration_since(previous).as_millis()).unwrap_or(u64::MAX);
                    Self::gap(&mut self.local_gaps, local);
                    if let (Some(previous), Some(current)) = (server, current) {
                        let remote =
                            (current + 24 * 60 * 60 * 1000 - previous) % (24 * 60 * 60 * 1000);
                        Self::gap(&mut self.server_gaps, remote);
                    }
                }
                self.last_frame = Some((now, current));
            }
            _ => {}
        }
    }
    // Fixed-size histograms; no frame IDs, desktop pixels or input contents.
    fn gap(buckets: &mut [u64; 5], milliseconds: u64) {
        if milliseconds < 1000 {
            let index = [10, 20, 40, 80].partition_point(|edge| milliseconds >= *edge);
            buckets[index] += 1;
        }
    }
    pub(crate) fn class(pdu: &crate::pdu::GfxPdu) -> usize {
        use crate::pdu::{Codec1Type, GfxPdu};
        match pdu {
            GfxPdu::WireToSurface1(wire) => match wire.codec_id {
                Codec1Type::ClearCodec => 0,
                Codec1Type::Uncompressed => 1,
                _ => 2,
            },
            GfxPdu::WireToSurface2(_) => 3,
            GfxPdu::EndFrame(_) => 4,
            GfxPdu::SurfaceToSurface(_) => 5,
            GfxPdu::SolidFill(_) | GfxPdu::CacheToSurface(_) | GfxPdu::SurfaceToCache(_) => 6,
            _ => 7,
        }
    }
    pub(crate) fn handled(&mut self, class: usize, elapsed: Duration) {
        self.commands[class] += elapsed;
        self.command_counts[class] += 1;
    }
    pub(crate) fn record(&mut self, decompress: Duration, parse: Duration, handle: Duration) {
        self.decompress += decompress;
        self.parse += parse;
        self.handle += handle;
        self.messages += 1;
        if self.since.elapsed() < Duration::from_secs(1) {
            return;
        }
        if self.records.len() == 128 {
            self.records.pop_front();
        }
        self.records.push_back(format!(
            "RDP_GFX seconds={:.3} messages={} decompress_ms={:.3} parse_ms={:.3} handle_ms={:.3} surface1={} progressive={} frames={}",
            self.since.elapsed().as_secs_f64(),
            self.messages,
            self.decompress.as_secs_f64() * 1000.,
            self.parse.as_secs_f64() * 1000.,
            self.handle.as_secs_f64() * 1000.,
            self.surface1,
            self.progressive,
            self.frames,
        ));
        if self.records.len() == 128 {
            self.records.pop_front();
        }
        self.records.push_back(format!(
            "RDP_GFX_DETAIL clear_ms={:.3} raw_ms={:.3} other_codec_ms={:.3} progressive_ms={:.3} commit_ms={:.3} surface_copy_ms={:.3} fill_cache_ms={:.3} other_ms={:.3} counts={:?} local_gap_buckets={:?} server_gap_buckets={:?} missing_timestamps={}",
            self.commands[0].as_secs_f64() * 1000.,
            self.commands[1].as_secs_f64() * 1000.,
            self.commands[2].as_secs_f64() * 1000.,
            self.commands[3].as_secs_f64() * 1000.,
            self.commands[4].as_secs_f64() * 1000.,
            self.commands[5].as_secs_f64() * 1000.,
            self.commands[6].as_secs_f64() * 1000.,
            self.commands[7].as_secs_f64() * 1000.,
            self.command_counts,
            self.local_gaps,
            self.server_gaps,
            self.missing_timestamps,
        ));
        self.since = Instant::now();
        self.decompress = Duration::ZERO;
        self.parse = Duration::ZERO;
        self.handle = Duration::ZERO;
        self.messages = 0;
        self.progressive = 0;
        self.surface1 = 0;
        self.frames = 0;
        self.commands = [Duration::ZERO; 8];
        self.command_counts = [0; 8];
        self.local_gaps = [0; 5];
        self.server_gaps = [0; 5];
        self.missing_timestamps = 0;
    }
}

impl Drop for Timing {
    fn drop(&mut self) {
        use std::io::Write as _;
        let stderr = std::io::stderr();
        let mut output = std::io::BufWriter::with_capacity(64 * 1024, stderr.lock());
        for record in &self.records {
            let _ = writeln!(output, "{record}");
        }
        let _ = output.flush();
    }
}
