//! Fixed-size opt-in counters. No encoded bitmaps or desktop pixels are retained.
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct Sample {
    pub total: Duration,
    pub coefficients: Duration,
    pub reconstruction: Duration,
    pub updated: usize,
    pub reconstructed: usize,
    pub reduced: usize,
}
pub(super) struct Timing {
    since: Instant,
    messages: usize,
    sample: Sample,
    records: std::collections::VecDeque<String>,
}
impl Timing {
    pub(super) fn enabled() -> Option<Self> {
        (std::env::var("TOMATO_RDP_PIPELINE_TIMING").as_deref() == Ok("1")).then(|| Self {
            since: Instant::now(),
            messages: 0,
            sample: Sample::default(),
            records: std::collections::VecDeque::new(),
        })
    }
    pub(super) fn record(&mut self, sample: Sample) {
        self.messages += 1;
        self.sample.total += sample.total;
        self.sample.coefficients += sample.coefficients;
        self.sample.reconstruction += sample.reconstruction;
        self.sample.updated += sample.updated;
        self.sample.reconstructed += sample.reconstructed;
        self.sample.reduced += sample.reduced;
        if self.since.elapsed() < Duration::from_secs(1) {
            return;
        }
        if self.records.len() == 128 {
            self.records.pop_front();
        }
        self.records.push_back(format!(
            "RDP_PROGRESSIVE seconds={:.3} messages={} total_ms={:.3} coefficients_ms={:.3} reconstruction_ms={:.3} updated_tiles={} reconstructed_tiles={} reduced_messages={}",
            self.since.elapsed().as_secs_f64(),
            self.messages,
            self.sample.total.as_secs_f64() * 1000.,
            self.sample.coefficients.as_secs_f64() * 1000.,
            self.sample.reconstruction.as_secs_f64() * 1000.,
            self.sample.updated,
            self.sample.reconstructed,
            self.sample.reduced
        ));
        self.since = Instant::now();
        self.messages = 0;
        self.sample = Sample::default();
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
