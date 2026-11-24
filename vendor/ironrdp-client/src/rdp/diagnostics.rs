//! Opt-in, bounded pipeline timing. Never logs transport or desktop contents.
use std::time::{Duration, Instant};

pub(super) struct Pipeline {
    since: Instant,
    pdus: u64,
    bytes: usize,
    decode: Duration,
    worst_decode: Duration,
    copies: Duration,
    output_wait: Duration,
    pictures: u64,
    records: std::collections::VecDeque<String>,
}

impl Pipeline {
    pub(super) fn enabled() -> Option<Self> {
        (std::env::var("TOMATO_RDP_PIPELINE_TIMING").as_deref() == Ok("1")).then(|| Self {
            since: Instant::now(),
            pdus: 0,
            bytes: 0,
            decode: Duration::ZERO,
            worst_decode: Duration::ZERO,
            copies: Duration::ZERO,
            output_wait: Duration::ZERO,
            pictures: 0,
            records: std::collections::VecDeque::new(),
        })
    }

    pub(super) fn decoded(&mut self, bytes: usize, elapsed: Duration) {
        self.pdus += 1;
        self.bytes += bytes;
        self.decode += elapsed;
        self.worst_decode = self.worst_decode.max(elapsed);
    }

    pub(super) fn presented(&mut self, copied: Duration, waited: Duration) {
        self.copies += copied;
        self.output_wait += waited;
        self.pictures += 1;
    }

    pub(super) fn report(&mut self) {
        let elapsed = self.since.elapsed();
        if elapsed < Duration::from_secs(1) {
            return;
        }
        if self.records.len() == 128 {
            self.records.pop_front();
        }
        self.records.push_back(format!(
            "RDP_PIPELINE seconds={:.3} pdus={} bytes={} decode_ms={:.3} worst_decode_ms={:.3} copy_ms={:.3} output_wait_ms={:.3} pictures={}",
            elapsed.as_secs_f64(),
            self.pdus,
            self.bytes,
            self.decode.as_secs_f64() * 1000.,
            self.worst_decode.as_secs_f64() * 1000.,
            self.copies.as_secs_f64() * 1000.,
            self.output_wait.as_secs_f64() * 1000.,
            self.pictures,
        ));
        self.since = Instant::now();
        self.pdus = 0;
        self.bytes = 0;
        self.decode = Duration::ZERO;
        self.worst_decode = Duration::ZERO;
        self.copies = Duration::ZERO;
        self.output_wait = Duration::ZERO;
        self.pictures = 0;
    }
}

impl Drop for Pipeline {
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
