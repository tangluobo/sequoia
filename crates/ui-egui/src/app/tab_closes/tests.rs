use super::*;
use anyhow::Context;
fn frame(app: &mut TomatoApp, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
    let ctx = app.context.clone();
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100., 750.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            app.workspace_view(ui, true);
            app.confirm_dialog(&ctx);
        },
    );
    let labels = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some((t.galley.job.text.clone(), t.pos + t.galley.size() * 0.5))
            } else {
                None
            }
        })
        .collect();
    out.textures_delta.clear();
    labels
}
fn click(app: &mut TomatoApp, point: egui::Pos2, button: egui::PointerButton) {
    for pressed in [true, false] {
        frame(
            app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    frame(app, vec![]);
}
fn menu(app: &mut TomatoApp, name: &str, label: &str) -> anyhow::Result<()> {
    let point = frame(app, vec![])
        .into_iter()
        .find(|(text, _)| text == name)
        .context("tab missing")?
        .1;
    click(app, point, egui::PointerButton::Secondary);
    let point = frame(app, vec![])
        .into_iter()
        .find(|(text, _)| text == label)
        .context("close menu missing")?
        .1;
    click(app, point, egui::PointerButton::Primary);
    Ok(())
}
#[test]
fn actual_tab_menus_close_sides_skip_pinned_objects_and_keep_cancelled_dirty_documents()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(dir.path().join("connections.json")),
    );
    let mut ids = Vec::new();
    for name in ["left", "pinned", "center", "right"] {
        let mut tab = tabs::create_tab(Connection::new("SQLITE"), None)?;
        tab.title = name.into();
        if name == "pinned"
            && let Panel::Sql { workbench, .. } = &mut tab.panel
        {
            workbench.browser = Some(Box::default());
        }
        ids.push(tab.id.clone());
        app.tabs.push(tab);
    }
    menu(&mut app, "center", "关闭右侧")?;
    anyhow::ensure!(!app.tabs.iter().any(|t| t.id == ids[3]));
    anyhow::ensure!(app.tabs.len() == 3);
    menu(&mut app, "center", "关闭左侧")?;
    anyhow::ensure!(!app.tabs.iter().any(|t| t.id == ids[0]));
    anyhow::ensure!(app.tabs.iter().any(|t| t.id == ids[1]));
    let mut dirty = tabs::create_tab(Connection::new("SQLITE"), None)?;
    dirty.title = "owned document".into();
    dirty.panel = Panel::Editor {
        path: dir.path().join("owned.txt"),
        text: "unsaved".into(),
        saved: String::new(),
        markdown: Box::default(),
    };
    let dirty_id = dirty.id.clone();
    app.tabs.insert(0, dirty);
    menu(&mut app, "center", "关闭全部")?;
    anyhow::ensure!(matches!(&app.confirmation,Some(Confirmation::Close(id))if id==&dirty_id));
    anyhow::ensure!(app.tabs.iter().any(|t| t.id == ids[2]));
    let cancel = frame(&mut app, vec![])
        .into_iter()
        .find(|(text, _)| text == "取消")
        .context("dirty close cancel missing")?
        .1;
    click(&mut app, cancel, egui::PointerButton::Primary);
    app.continue_tab_closes();
    anyhow::ensure!(
        app.tabs.iter().any(|t| t.id == dirty_id)
            && app.tabs.iter().any(|t| t.id == ids[1])
            && !app.tabs.iter().any(|t| t.id == ids[2])
    );
    anyhow::ensure!(
        !dir.path().join("owned.txt").exists(),
        "cancel must not write the document"
    );
    Ok(())
}

#[test]
fn close_all_waits_for_actual_file_save_uses_snapshot_and_keeps_a_cancelled_query()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let mut anchor = tabs::create_tab(Connection::new("SQLITE"), None)?;
    anchor.title = "anchor".into();
    let anchor_id = anchor.id.clone();
    let mut editor = tabs::create_tab(Connection::new("SQLITE"), None)?;
    let editor_id = editor.id.clone();
    let path = directory.path().join("saved.md");
    std::fs::write(&path, "old")?;
    editor.title = "saved.md".into();
    editor.panel = Panel::Editor {
        path: path.clone(),
        text: "保存中文\nnew".into(),
        saved: "old".into(),
        markdown: Box::default(),
    };
    let mut query = tabs::create_tab(Connection::new("SQLITE"), None)?;
    let query_id = query.id.clone();
    if let Panel::Sql { sql, workbench, .. } = &mut query.panel {
        *sql = "SELECT 'keep draft'".into();
        workbench.document = Some(crate::tabs::queries::Document::new(
            tomato_core::queries::Scope {
                connection: "owned".into(),
                database: "main".into(),
                path: String::new(),
            },
            sql,
        ));
    }
    app.tabs = vec![editor, query, anchor];
    menu(&mut app, "anchor", "关闭全部")?;
    let save = frame(&mut app, vec![])
        .into_iter()
        .find(|(label, _)| label == "保存")
        .context("save button")?
        .1;
    click(&mut app, save, egui::PointerButton::Primary);
    anyhow::ensure!(
        app.tabs.iter().any(|tab| tab.id == editor_id),
        "save is asynchronous"
    );
    let mut new_tab = tabs::create_tab(Connection::new("SQLITE"), None)?;
    new_tab.title = "opened while saving".into();
    let new_id = new_tab.id.clone();
    app.tabs.push(new_tab);
    // A competing menu must not replace the first queue while its save is in flight.
    app.close_tab_group(&new_id, Side::All);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !matches!(&app.confirmation,Some(Confirmation::Close(id))if id==&query_id) {
        app.logic(&ctx);
        anyhow::ensure!(
            std::time::Instant::now() < deadline && app.error.is_none(),
            "save/close failed: {:?}",
            app.error
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    anyhow::ensure!(
        std::fs::read_to_string(&path)? == "保存中文\nnew"
            && !app.tabs.iter().any(|tab| tab.id == editor_id),
        "close follows durable save"
    );
    let cancel = frame(&mut app, vec![])
        .into_iter()
        .find(|(label, _)| label == "取消")
        .context("cancel button")?
        .1;
    click(&mut app, cancel, egui::PointerButton::Primary);
    app.continue_tab_closes();
    anyhow::ensure!(
        app.tabs.iter().any(|tab| tab.id == query_id)
            && app.tabs.iter().any(|tab| tab.id == new_id)
            && !app.tabs.iter().any(|tab| tab.id == anchor_id),
        "snapshot and per-tab cancellation"
    );
    Ok(())
}
