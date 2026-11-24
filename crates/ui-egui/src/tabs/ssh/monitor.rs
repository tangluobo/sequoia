//! Bounded, timestamped samples for the original one-hour SSH monitor.
use super::*;
use std::time::{SystemTime, UNIX_EPOCH};

const HISTORY_LIMIT: usize = 3600;
const RATE_SCALE: f32 = 1_048_576.;

pub struct Sample {
    pub values: [f32; 6],
    pub timestamp: f64,
}
pub struct Counters {
    pub time: Instant,
    pub total: u64,
    pub idle: u64,
    pub memory: f32,
    pub received: u64,
    pub sent: u64,
    pub read: u64,
    pub written: u64,
}
impl Workspace {
    pub(super) fn reset_monitor(&mut self) {
        self.counters = None;
        self.history.clear();
        self.metrics = [0.; 6];
        self.last_monitor = Instant::now() - Duration::from_secs(2);
    }
    pub(super) fn update_monitor(&mut self, text: &str) {
        let Some(current) = parse_monitor(text) else {
            return;
        };
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs_f64();
        self.record_monitor(current, timestamp);
    }
    fn record_monitor(&mut self, current: Counters, timestamp: f64) {
        let mut values = [0., current.memory, 0., 0., 0., 0.];
        if let Some(previous) = &self.counters {
            let dt = current
                .time
                .saturating_duration_since(previous.time)
                .as_secs_f32()
                .max(0.01);
            let total = current.total.saturating_sub(previous.total);
            let idle = current.idle.saturating_sub(previous.idle);
            if total > 0 {
                values[0] = (1. - idle.min(total) as f32 / total as f32) * 100.;
            }
            for (index, new, old) in [
                (2, current.received, previous.received),
                (3, current.sent, previous.sent),
                (4, current.read, previous.read),
                (5, current.written, previous.written),
            ] {
                values[index] = new.saturating_sub(old) as f32 / dt;
            }
        }
        let previous = self.history.back();
        let mut plotted = [0.; 6];
        for (i, value) in values.into_iter().enumerate() {
            let percent = if i < 2 {
                value
            } else {
                value / RATE_SCALE * 100.
            }
            .clamp(0., 100.);
            plotted[i] = previous.map_or(percent, |old| old.values[i] * 0.7 + percent * 0.3);
            // Java shows the current transfer rate, and smooths only its graph.
            self.metrics[i] = if i < 2 { plotted[i] } else { value };
        }
        self.counters = Some(current);
        if self.history.len() >= HISTORY_LIMIT {
            self.history.pop_front();
        }
        let timestamp = self
            .history
            .back()
            .map_or(timestamp, |old| old.timestamp.max(timestamp));
        self.history.push_back(Sample {
            values: plotted,
            timestamp,
        });
    }
}
fn parse_monitor(text: &str) -> Option<Counters> {
    let sections: Vec<_> = text.split("__SEP__").collect();
    if sections.len() < 4 {
        return None;
    }
    let cpu: Vec<u64> = sections[0]
        .split_whitespace()
        .skip(1)
        .filter_map(|n| n.parse().ok())
        .collect();
    if cpu.len() < 4 {
        return None;
    }
    let mem: BTreeMap<_, u64> = sections[1]
        .lines()
        .filter_map(|l| {
            let mut p = l.split_whitespace();
            Some((p.next()?.trim_end_matches(':'), p.next()?.parse().ok()?))
        })
        .collect();
    let total = *mem.get("MemTotal")?;
    let available = mem.get("MemAvailable").copied().unwrap_or_else(|| {
        mem.get("MemFree").copied().unwrap_or(0)
            + mem.get("Buffers").copied().unwrap_or(0)
            + mem.get("Cached").copied().unwrap_or(0)
    });
    let (mut rx, mut tx) = (0_u64, 0_u64);
    for l in sections[2].lines() {
        if let Some((interface, data)) = l.split_once(':')
            && interface.trim() != "lo"
        {
            let p: Vec<u64> = data
                .split_whitespace()
                .filter_map(|p| p.parse().ok())
                .collect();
            if p.len() > 8 {
                rx = rx.saturating_add(p[0]);
                tx = tx.saturating_add(p[8]);
            }
        }
    }
    let (mut read, mut written) = (0_u64, 0_u64);
    for l in sections[3].lines() {
        let p: Vec<_> = l.split_whitespace().collect();
        if p.len() > 9
            && !(p[2].starts_with("loop") || p[2].starts_with("ram"))
            && sections
                .get(4)
                .is_none_or(|devices| devices.split_whitespace().any(|name| name == p[2]))
        {
            read = read.saturating_add(p[5].parse::<u64>().unwrap_or(0).saturating_mul(512));
            written = written.saturating_add(p[9].parse::<u64>().unwrap_or(0).saturating_mul(512));
        }
    }
    Some(Counters {
        time: Instant::now(),
        total: cpu
            .iter()
            .take(8)
            .fold(0_u64, |total, part| total.saturating_add(*part)),
        idle: cpu[3].saturating_add(cpu.get(4).copied().unwrap_or(0)),
        memory: 100. * total.saturating_sub(available) as f32 / total.max(1) as f32,
        received: rx,
        sent: tx,
        read,
        written,
    })
}

#[cfg(test)]
mod tests;
