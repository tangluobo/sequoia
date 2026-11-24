use super::*;

fn counters(time: Instant, total: u64, idle: u64, bytes: u64) -> Counters {
    Counters {
        time,
        total,
        idle,
        memory: 50.,
        received: bytes,
        sent: bytes / 2,
        read: bytes * 2,
        written: bytes * 3,
    }
}
#[test]
fn delayed_samples_show_current_rates_and_smooth_capped_graphs() {
    let mut workspace = Workspace::default();
    let start = Instant::now();
    workspace.record_monitor(counters(start, 100, 50, 5000), 1000.);
    workspace.record_monitor(
        counters(
            start + Duration::from_secs(4),
            500,
            150,
            5000 + 4 * 4_194_304,
        ),
        1004.,
    );
    assert_eq!(workspace.metrics[0], 22.5);
    assert_eq!(workspace.metrics[2], 4_194_304.);
    assert_eq!(workspace.metrics[3], 2_097_152.);
    assert!((workspace.history.back().unwrap().values[2] - 30.).abs() < 0.0001);
    assert_eq!(workspace.history.back().unwrap().timestamp, 1004.);
    // A fast transfer must not leave the graph pinned at 100% after it ends.
    workspace.record_monitor(
        counters(
            start + Duration::from_secs(5),
            600,
            200,
            5000 + 4 * 4_194_304,
        ),
        1005.,
    );
    assert_eq!(workspace.metrics[2], 0.);
    assert!((workspace.history.back().unwrap().values[2] - 21.).abs() < 0.0001);
}
#[test]
fn one_hour_history_is_bounded_and_reconnect_drops_old_baselines() {
    let mut workspace = Workspace::default();
    let start = Instant::now();
    for index in 0..3700 {
        workspace.record_monitor(
            counters(
                start + Duration::from_secs(index),
                index * 10,
                index * 5,
                index * 1000,
            ),
            1000. + index as f64,
        );
    }
    assert_eq!(workspace.history.len(), 3600);
    assert_eq!(workspace.history.front().unwrap().timestamp, 1100.);
    assert_eq!(workspace.history.back().unwrap().timestamp, 4699.);
    assert!(workspace.history.capacity() <= 4096);
    assert!(std::mem::size_of::<Sample>() * workspace.history.capacity() <= 131_072);
    workspace.reconnected();
    assert!(workspace.history.is_empty());
    assert!(workspace.counters.is_none());
    assert_eq!(workspace.metrics, [0.; 6]);
    workspace.record_monitor(
        counters(start + Duration::from_secs(4000), 30, 10, 100),
        5000.,
    );
    assert_eq!(workspace.metrics[2], 0.);
    assert_eq!(workspace.metrics[1], 50.);
}
#[test]
fn physical_disks_are_counted_once_and_loopback_is_excluded() {
    let sample = parse_monitor(
        "cpu 100 0 100 600 100 0 0 0 999 0
__SEP__
MemTotal: 1000 kB
MemAvailable: 400 kB
__SEP__
lo: 9999 0 0 0 0 0 0 0 9999
eth0: 1200 0 0 0 0 0 0 0 3400
__SEP__
8 0 sda 1 0 100 0 1 0 200 0
8 1 sda1 1 0 100 0 1 0 200 0
253 0 dm-0 1 0 100 0 1 0 200 0
259 0 nvme0n1 1 0 50 0 1 0 60 0
7 0 loop0 1 0 9999 0 1 0 9999 0
__SEP__
sda
nvme0n1
",
    )
    .unwrap();
    assert_eq!(sample.total, 900);
    assert_eq!(sample.idle, 700);
    assert_eq!(sample.memory, 60.);
    assert_eq!(sample.received, 1200);
    assert_eq!(sample.sent, 3400);
    assert_eq!(sample.read, 150 * 512);
    assert_eq!(sample.written, 260 * 512);
}
#[test]
fn counter_reset_and_wall_clock_change_do_not_create_spikes_or_reverse_time() {
    let mut workspace = Workspace::default();
    let start = Instant::now();
    workspace.record_monitor(counters(start, 1000, 500, 50_000), 1000.);
    workspace.record_monitor(counters(start + Duration::from_secs(1), 10, 5, 50), 900.);
    assert_eq!(workspace.metrics[2], 0.);
    assert_eq!(workspace.metrics[0], 0.);
    assert_eq!(workspace.history.back().unwrap().timestamp, 1000.);
    let samples = workspace.history.len();
    workspace.update_monitor("not a monitoring result");
    assert_eq!(workspace.history.len(), samples);
}
