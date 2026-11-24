use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn commands(exe: &Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(exe.with_extension("commands.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}
fn touches(exe: &Path) -> Vec<Vec<u8>> {
    std::fs::read_to_string(exe.with_extension("scrcpy-input.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Vec<u8>>(line).ok())
        .filter(|v| v.len() == 32 && v[0] == 2)
        .collect()
}
async fn until(mut predicate: impl FnMut() -> bool) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(3)).await;
        }
    })
    .await
    .context("Shared ADB fixture did not reach the expected state")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires owned adb_fixture; real shared decoder, per-view touches, encoder changes and final cleanup"]
async fn one_device_shares_pixels_but_keeps_views_inputs_reconfiguration_and_lifetimes_independent()
-> Result<()> {
    let root = tempfile::tempdir()?;
    let exe = Fixture::start(root.path())?;
    let original = exe.connection();
    let mut another = original.clone();
    another.id = uuid::Uuid::new_v4().to_string();
    another.set("preservedLegacyField", "中文");
    another.set("adbExecutablePath", "missing/another-legacy-adb");
    let before = [
        serde_json::to_value(&original)?,
        serde_json::to_value(&another)?,
    ];
    let runtime = tokio::runtime::Handle::current();
    let first = DesktopSession::start(&original, &runtime)?;
    let second = DesktopSession::start(&another, &runtime)?;
    let first_wakes = Arc::new(AtomicUsize::new(0));
    let second_wakes = Arc::new(AtomicUsize::new(0));
    for (session, count) in [
        (&first, first_wakes.clone()),
        (&second, second_wakes.clone()),
    ] {
        session.set_yuv_output(true);
        session.on_output(move || {
            count.fetch_add(1, Ordering::Relaxed);
        });
    }
    until(|| {
        if let (Some(a), Some(b)) = (first.take_yuv_snapshot(), second.take_yuv_snapshot()) {
            assert_eq!((a.width, a.height), (64, 48));
            Arc::ptr_eq(&a, &b)
        } else {
            false
        }
    })
    .await?;
    let starts = || {
        commands(&exe)
            .iter()
            .filter(|c| c.first().is_some_and(|v| v == "connect"))
            .count()
    };
    ensure!(starts() == 1, "Two views created duplicate ADB transports");
    ensure!(
        first_wakes.load(Ordering::Relaxed) > 0 && second_wakes.load(Ordering::Relaxed) > 0,
        "A view did not receive decoder notifications"
    );
    for (session, x) in [(&first, 3), (&second, 13)] {
        session.send(Input::Resize {
            width: 64,
            height: 48,
        })?;
        session.send(Input::Pointer {
            x,
            y: 4,
            button: Some(1),
            down: true,
            wheel: 0,
        })?;
        session.send(Input::Pointer {
            x: x + 2,
            y: 6,
            button: None,
            down: false,
            wheel: 0,
        })?;
    }
    until(|| touches(&exe).iter().filter(|v| v[1] == 0).count() == 2).await?;
    let downs: Vec<_> = touches(&exe).into_iter().filter(|v| v[1] == 0).collect();
    let first_id = u64::from_be_bytes(
        downs
            .iter()
            .find(|v| u32::from_be_bytes(v[10..14].try_into().unwrap()) == 3)
            .unwrap()[2..10]
            .try_into()?,
    );
    let second_id = u64::from_be_bytes(
        downs
            .iter()
            .find(|v| u32::from_be_bytes(v[10..14].try_into().unwrap()) == 13)
            .unwrap()[2..10]
            .try_into()?,
    );
    ensure!(
        first_id != second_id,
        "Views reused a scrcpy touch identifier"
    );
    drop(first);
    until(|| {
        touches(&exe)
            .iter()
            .any(|v| v[1] == 1 && u64::from_be_bytes(v[2..10].try_into().unwrap()) == first_id)
    })
    .await?;
    ensure!(
        second.status() == Status::Ready && starts() == 1,
        "Closing a view disconnected its sibling"
    );
    second.send(Input::Pointer {
        x: 15,
        y: 6,
        button: Some(1),
        down: false,
        wheel: 0,
    })?;
    let third = DesktopSession::start(&original, &runtime)?;
    third.set_yuv_output(true);
    until(|| third.take_yuv_snapshot().is_some()).await?;
    ensure!(
        starts() == 1,
        "Opening a third view restarted an existing device"
    );

    let mut changed = another.clone();
    tomato_services::remote_desktop::AdbVideoQuality::Fast.apply(&mut changed);
    changed.set("adbDisplayMode", "H264");
    second.reconfigure_adb(&changed)?;
    until(|| {
        starts() == 2 && third.status() == Status::Ready && third.take_yuv_snapshot().is_some()
    })
    .await?;
    let mut shown = original.clone();
    third.apply_adb_settings(&mut shown);
    ensure!(
        shown.text("adbVideoQuality") == "fast" && shown.number("adbVideoMaxSize") == 1024,
        "Encoder settings did not reach the other view"
    );
    let launches = commands(&exe);
    ensure!(
        launches
            .iter()
            .any(|c| c.iter().any(|v| v == "max_size=1024")),
        "Actual scrcpy process did not use the new encoder size"
    );
    shown.set("adbDisplayMode", "PNG");
    third.reconfigure_adb(&shown)?;
    until(|| {
        second
            .take_frame()
            .is_some_and(|f| (f.width, f.height) == (96, 64))
            && third
                .take_frame()
                .is_some_and(|f| (f.width, f.height) == (96, 64))
    })
    .await?;
    ensure!(starts() == 3, "Mode switch started more than one transport");
    drop(third);
    until(|| second.take_frame().is_some()).await?;
    drop(second);
    tokio::time::timeout(
        Duration::from_secs(6),
        tomato_services::remote_desktop::wait_adb_cleanup(),
    )
    .await?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let marker = exe.with_extension("heartbeat");
    let stopped = std::fs::read(&marker)?;
    tokio::time::sleep(Duration::from_millis(150)).await;
    ensure!(
        std::fs::read(marker)? == stopped,
        "The final view retained its screenshot process"
    );
    let log = commands(&exe);
    let paths = |action: &str| {
        let mut jars = log
            .iter()
            .filter(|command| command.first().is_some_and(|value| value == action))
            .filter_map(|command| command.get(1))
            .collect::<Vec<_>>();
        jars.sort();
        jars
    };
    let pushed = paths("push");
    ensure!(
        pushed.len() == 2 && pushed == paths("remove"),
        "Owned encoder files leaked"
    );
    ensure!(
        before
            == [
                serde_json::to_value(&original)?,
                serde_json::to_value(&another)?
            ],
        "Legacy connection was modified"
    );
    println!(
        "Shared ADB: one encoder for multiple views, identical Arc pixels, distinct touches, close release, synchronized H264/PNG and owned cleanup verified"
    );
    Ok(())
}
