use super::*;
use std::time::{Duration, Instant};
fn wait(app: &mut TomatoApp, done: impl Fn(&TomatoApp) -> bool) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let ctx = app.context.clone();
    while Instant::now() < deadline {
        app.logic(&ctx);
        if done(app) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    anyhow::bail!("fixture job did not complete")
}
#[test]
fn real_ai_request_survives_other_queries_inserts_at_current_caret_and_never_executes()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(directory.path().join("connections.json"));
    let mut store = repository.load()?;
    let connection = Connection::new("SQLITE");
    store.connections.push(connection.clone());
    repository.save(&store)?;
    let original = std::fs::read(directory.path().join("connections.json"))?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let (observed, received) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || -> anyhow::Result<()> {
        use std::io::{Read, Write};
        let (mut socket, _) = listener.accept()?;
        socket.set_read_timeout(Some(Duration::from_secs(15)))?;
        let mut bytes = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            let count = socket.read(&mut buffer)?;
            anyhow::ensure!(count > 0, "closed request");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&bytes[..end])?;
                let size = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + size {
                    observed.send(serde_json::from_slice::<serde_json::Value>(
                        &bytes[end + 4..end + 4 + size],
                    )?)?;
                    break;
                }
            }
            anyhow::ensure!(bytes.len() < 64 * 1024, "oversize request");
        }
        released.recv_timeout(Duration::from_secs(15))?;
        let body=serde_json::json!({"choices":[{"message":{"content":"CREATE TABLE ai_should_not_execute(id INTEGER)"}}]}).to_string();
        socket.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )?;
        Ok(())
    });
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    app.open_connection(connection.clone());
    wait(&mut app, |app| app.pending.is_empty())?;
    let id = app.active.clone().unwrap();
    for (name, value) in [
        ("aiApiBaseUrl", format!("http://{address}/v1")),
        ("aiModel", "fixture-model".into()),
        ("aiApiKey", "fixture-key".into()),
    ] {
        app.settings.extra.insert(name.into(), value.into());
    }
    if let Panel::Sql { sql, workbench, .. } = &mut app.tabs[0].panel {
        *sql = "-- ai: 人数\nSELECT 7;".into();
        workbench.completion.selection = egui::text::CCursorRange::one(egui::text::CCursor::new(6));
    }
    app.request_ai_sql(&id, false);
    app.request_ai_sql(&id, false);
    assert_eq!(
        app.pending
            .values()
            .filter(|pending| matches!(pending.purpose, Purpose::AiSql(_)))
            .count(),
        1
    );
    assert!(!app.tabs[0].busy);
    let request = received.recv_timeout(Duration::from_secs(15))?;
    assert_eq!(request["messages"][1]["content"], "需求：人数");
    assert!(!request.to_string().contains("fixture-key"));
    assert!(!request.to_string().contains("SELECT 7;"));
    app.submit(
        &id,
        Command::Sql {
            connection: connection.clone(),
            sql: "SELECT 11".into(),
        },
        Purpose::Normal,
    );
    wait(&mut app, |app| !app.tabs[0].busy)?;
    assert!(
        app.pending
            .values()
            .any(|pending| matches!(pending.purpose, Purpose::AiSql(_)))
    );
    if let Panel::Sql { sql, workbench, .. } = &mut app.tabs[0].panel {
        *sql = "正在编辑😀\n".into();
        workbench.completion.selection = egui::text::CCursorRange::one(egui::text::CCursor::new(2));
    }
    release.send(())?;
    wait(&mut app, |app| app.pending.is_empty())?;
    server.join().unwrap()?;
    if let Panel::Sql {
        sql,
        workbench,
        results,
        ..
    } = &app.tabs[0].panel
    {
        assert_eq!(
            sql,
            "正在\nCREATE TABLE ai_should_not_execute(id INTEGER)\n编辑😀\n"
        );
        assert!(!workbench.ai.pending());
        assert!(workbench.document.as_ref().unwrap().dirty());
        assert_eq!(results[0].rows[0][0], "11");
        let range = workbench.completion.selection.as_sorted_char_range();
        assert_eq!(
            (usize::from(range.start), usize::from(range.end)),
            (
                3,
                3 + "CREATE TABLE ai_should_not_execute(id INTEGER)"
                    .chars()
                    .count()
            )
        );
    } else {
        panic!("SQL tab");
    }
    app.submit(
        &id,
        Command::Sql {
            connection,
            sql: "SELECT count(*) FROM sqlite_master WHERE name='ai_should_not_execute'".into(),
        },
        Purpose::Normal,
    );
    wait(&mut app, |app| app.pending.is_empty())?;
    assert!(matches!(&app.tabs[0].panel,Panel::Sql{results,..}if results[0].rows[0][0]=="0"));
    assert_eq!(
        std::fs::read(directory.path().join("connections.json"))?,
        original
    );
    Ok(())
}
