//! Standalone owned-file probes of the real worker/job and cancellation paths.
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tomato_core::data_import::{Format, OdbcTable, Source};
use tomato_services::data_import::{self, Control};
fn native_helper_started(cache: &Path) -> bool {
    let Ok(roots) = cache.read_dir() else {
        return false;
    };
    for root in roots.flatten() {
        let Ok(children) = root.path().read_dir() else {
            continue;
        };
        for child in children.flatten() {
            let p = child.path();
            if p.is_dir()
                && ["tables.bin", "rows.bin", "result.bin"]
                    .iter()
                    .any(|f| p.join(f).exists())
            {
                return true;
            }
        }
    }
    false
}
fn main() -> Result<()> {
    if let Some(code) = data_import::worker_entry() {
        std::process::exit(code);
    }
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(args.len() == 3, "format input-file repeat|cancel");
    let cache = std::env::temp_dir();
    ensure!(
        cache.file_name().is_some_and(|p| p
            .to_string_lossy()
            .starts_with("tomato-import-owned-cache-")),
        "use an owned temporary cache"
    );
    ensure!(cache.read_dir()?.next().is_none(), "cache must start empty");
    let path = PathBuf::from(&args[1]);
    ensure!(
        path.components().any(|p| p
            .as_os_str()
            .to_string_lossy()
            .starts_with("tomato-import-fixture-"))
            && path.is_file(),
        "use an owned native fixture source"
    );
    let mut source = Source {
        format: match args[0].as_str() {
            "csv" => Format::Csv,
            "accdb" => Format::Access,
            "odbc" => Format::Odbc,
            _ => anyhow::bail!("unsupported owned fixture"),
        },
        files: vec![path.clone()],
        delimiter: ',',
        ..Default::default()
    };
    if source.format == Format::Access {
        source.first_row = 1;
    }
    if source.format == Format::Odbc {
        source.first_row = 1;
        source.odbc.use_32bit = true;
        source.odbc.connection_string = format!(
            "Driver={{Microsoft Text Driver (*.txt; *.csv)}};Dbq={};Extensions=csv;ReadOnly=True;",
            path.parent().context("source directory")?.display()
        );
        source.odbc.tables = vec![OdbcTable {
            catalog: String::new(),
            schema: String::new(),
            name: path
                .file_name()
                .context("source name")?
                .to_string_lossy()
                .into_owned(),
        }];
    }
    let started = Instant::now();
    let cancel = args[2] == "cancel";
    let iterations = if cancel { 1 } else { 20 };
    let mut records = 0;
    for _ in 0..iterations {
        if cancel {
            let control = Control::default();
            let signal = control.clone();
            let source = source.clone();
            let worker = std::thread::spawn(move || data_import::prepare(source, &control));
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                if native_helper_started(&cache) {
                    break;
                }
                ensure!(
                    !worker.is_finished() && Instant::now() < deadline,
                    "native helper was not observed before cancellation"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            signal.cancel();
            let result = worker
                .join()
                .map_err(|_| anyhow::anyhow!("worker panicked"))?;
            ensure!(result.is_err(), "cancelled import unexpectedly completed");
        } else {
            let prepared = data_import::prepare(source.clone(), &Control::default())?;
            records += prepared.tables.iter().map(|t| t.rows).sum::<u64>();
            drop(prepared);
        }
        ensure!(
            cache.read_dir()?.next().is_none(),
            "source or nested native cache survived teardown"
        );
    }
    println!(
        "{}",
        serde_json::json!({"status":"passed","format":args[0],"cancelledDuringNativeHelper":cancel,"iterations":iterations,"records":records,"milliseconds":started.elapsed().as_millis(),"cacheEmpty":true,"savedHostsContacted":false})
    );
    Ok(())
}
