//! Explicit OS lifecycle probe: no topology, injected events, sockets or input logging.
#[cfg(windows)]
fn main() -> anyhow::Result<()> {
    let capture = tomato_native_input::start("isolated-probe".into(), Box::new(|_| true))?;
    capture.send(tomato_native_input::Update::Healthy);
    std::thread::sleep(std::time::Duration::from_millis(100));
    drop(capture);
    println!("Global input hooks installed and released; no input injected");
    Ok(())
}
#[cfg(not(windows))]
fn main() {
    eprintln!("The native input adapter requires Windows");
}
