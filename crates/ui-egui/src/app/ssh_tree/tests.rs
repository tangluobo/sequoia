use super::*;
fn app() -> anyhow::Result<(TomatoApp, tempfile::TempDir)> {
    let root = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    Ok((
        TomatoApp::with_repository(
            &egui::Context::default(),
            runtime,
            ConnectionFileRepository::new(root.path().join("connections.json")),
        ),
        root,
    ))
}
#[test]
fn duplicate_tree_request_during_authentication_does_not_open_another_password_prompt()
-> anyhow::Result<()> {
    let (mut app, _root) = app()?;
    let listener = app
        .runtime
        .as_ref()
        .unwrap()
        .block_on(tokio::net::TcpListener::bind("127.0.0.1:0"))?;
    let mut c = Connection::new("SSH");
    c.set("host", "127.0.0.1");
    c.set("username", "fixture");
    c.set("port", listener.local_addr()?.port());
    c.set("password", "fixture-only");
    app.store.connections.push(c.clone());
    app.ssh_tree_request(c.clone(), Operation::Query(Section::Docker));
    assert!(app.ssh_hosts[&c.id].job.is_some());
    c.options.remove("password");
    app.ssh_tree_request(c, Operation::Query(Section::Docker));
    assert!(app.credential.is_none());
    Ok(())
}
#[test]
fn transient_credentials_are_reused_only_for_the_same_public_connection_options()
-> anyhow::Result<()> {
    let (mut app, _root) = app()?;
    let mut c = Connection::new("SSH");
    c.set("host", "127.0.0.1");
    c.set("username", "fixture");
    c.set("password", "fixture-only");
    app.ssh_hosts.entry(c.id.clone()).or_default().credentials = Some(c.clone());
    c.options.remove("password");
    assert_eq!(
        app.cached_ssh_connection(c.clone()).text("password"),
        "fixture-only"
    );
    c.set("port", 2200);
    assert!(app.cached_ssh_connection(c).text("password").is_empty());
    Ok(())
}
