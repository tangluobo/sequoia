//! Opt-in native downloader diagnostics. Never opens the user's shared history.
use anyhow::{Context, Result, bail, ensure};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use tomato_services::downloader::{Action, Manager};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let profile = PathBuf::from(args.next().context("profile directory")?);
    let url = args.next().context("eD2k file URL")?;
    let mut public = false;
    let mut server = None;
    let mut resume = None;
    let mut seconds = 600u64;
    while let Some(option) = args.next() {
        match option.as_str() {
            "--allow-public" => public = true,
            "--server" => server = Some(args.next().context("loopback server")?),
            "--resume-metadata" => {
                resume = Some(PathBuf::from(args.next().context("metadata path")?))
            }
            "--seconds" => seconds = args.next().context("timeout")?.parse()?,
            _ => bail!("unknown option: {option}"),
        }
    }
    ensure!(
        profile.is_absolute()
            && profile
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-ed2k-probe-"))
            && !profile.exists(),
        "choose a fresh absolute tomato-ed2k-probe-* directory"
    );
    ensure!(
        (1..=1800).contains(&seconds),
        "timeout must be 1..1800 seconds"
    );
    ensure!(
        url.to_ascii_lowercase().starts_with("ed2k://|file|"),
        "requires an eD2k file URL"
    );
    tomato_services::downloader::validate_source(&url)?;
    let link = tomato_ed2k::parse_ed2k_link(&url).map_err(anyhow::Error::msg)?;
    ensure!(
        !link.file_name.contains(['/', '\\', ':']) && !link.file_name.chars().any(char::is_control),
        "requires a single safe file name"
    );
    ensure!(
        public != server.is_some(),
        "select either --allow-public or --server 127.0.0.1:port"
    );
    if let Some(server) = &server {
        let endpoint: std::net::SocketAddrV4 = server.parse()?;
        ensure!(
            endpoint.ip().is_loopback() && endpoint.port() != 0,
            "fixture server must be loopback"
        );
    }
    tracing_subscriber::fmt()
        .with_env_filter("tomato_ed2k=debug")
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|error| anyhow::anyhow!("logging: {error}"))?;
    tomato_services::init_crypto();
    let files = profile.join("files");
    std::fs::create_dir_all(profile.join("rust-ed2k"))?;
    std::fs::create_dir_all(&files)?;
    if let Some(server) = server {
        std::fs::write(
            profile.join("rust-ed2k/network.json"),
            serde_json::to_vec(
                &serde_json::json!({"servers":[server],"kad":false,"loopbackOnly":true}),
            )?,
        )?;
    }
    let target = files.join(&link.file_name);
    if let Some(resume) = resume {
        ensure!(
            std::fs::metadata(&resume)?.len() <= 2 * 1024 * 1024,
            "metadata exceeds 2 MiB"
        );
        let met: tomato_ed2k::partial::Met = serde_json::from_slice(&std::fs::read(resume)?)?;
        ensure!(
            met.version == 1
                && met.file_size == link.file_size
                && met.file_hash.eq_ignore_ascii_case(&link.file_hash),
            "metadata belongs to another file"
        );
        let hashes = met.decoded_hashset().context("cached part hashes")?;
        ensure!(
            hashes.len() == (link.file_size / tomato_ed2k::types::ED2K_CHUNK_SIZE + 1) as usize
                && tomato_ed2k::md4::md4(&hashes.iter().flatten().copied().collect::<Vec<_>>())
                    == link.file_hash_bytes,
            "invalid cached part hashset"
        );
        // Copy only metadata into our empty partial; never claim original data was copied.
        let copy = tomato_ed2k::partial::Met::new(
            link.file_hash_bytes,
            link.file_size,
            Some(&hashes),
            Vec::new(),
        );
        tomato_ed2k::partial::save_met(&tomato_ed2k::partial::met_path(&target), &copy)?;
        std::fs::File::create(tomato_ed2k::partial::part_path(&target))?;
        let mut owner = target.as_os_str().to_owned();
        owner.push(".tomato-ed2k.owner");
        std::fs::write(
            PathBuf::from(owner),
            serde_json::to_vec(
                &serde_json::json!({"version":1,"task":"legacy-before-java-rewrite","hash":link.file_hash,"size":link.file_size}),
            )?,
        )?;
    }
    let history = profile.join("downloads.json");
    std::fs::write(
        &history,
        serde_json::to_vec(
            &serde_json::json!({"version":1,"tasks":[{"url":url,"protocol":"ED2K","directory":files,"requestedThreads":8,"fileName":link.file_name,"totalBytes":link.file_size,"targetFile":target,"state":"WAITING","status":"等待中"}]}),
        )?,
    )?;
    let manager = Manager::open(history.clone(), || {});
    let started = Instant::now();
    let threshold = link.file_size.min(tomato_ed2k::types::ED2K_CHUNK_SIZE);
    let mut last = String::new();
    let mut row = None;
    let outcome = loop {
        let snapshot = manager.snapshot();
        ensure!(snapshot.error.is_none(), "actor: {:?}", snapshot.error);
        if let Some(current) = snapshot.rows.first() {
            let status = format!(
                "{} · {} verified bytes · {}",
                current.state, current.downloaded_bytes, current.status
            );
            if status != last {
                println!("{:.1}s {status}", started.elapsed().as_secs_f64());
                last = status;
            }
            row = Some(current.clone());
            if current.downloaded_bytes >= threshold || current.state == "COMPLETED" {
                break "verified-part";
            }
            if current.state == "FAILED" {
                break "failed";
            }
        }
        if started.elapsed() >= Duration::from_secs(seconds) {
            break "timeout";
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if let Some(current) = &row {
        manager.send(Action::Cancel(current.id.clone()))?;
        let closing = Instant::now();
        while closing.elapsed() < Duration::from_secs(5) {
            if manager
                .snapshot()
                .rows
                .iter()
                .all(|row| matches!(row.state.as_str(), "CANCELLED" | "COMPLETED" | "FAILED"))
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    drop(manager);
    let proof = serde_json::json!({"outcome":outcome,"profile":profile,"fileHash":link.file_hash,"fileSize":link.file_size,"verifiedBytes":row.as_ref().map_or(0,|row|row.downloaded_bytes),"elapsedSeconds":started.elapsed().as_secs_f64(),"publicNetworkRequested":public,"sharedHistoryWritten":false});
    std::fs::write(
        profile.join("probe-result.json"),
        serde_json::to_vec_pretty(&proof)?,
    )?;
    println!("{proof}");
    ensure!(
        outcome == "verified-part",
        "no verified file part received; inspect probe logs"
    );
    Ok(())
}
