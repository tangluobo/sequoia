//! Real Hive catalog/designer/data/query controllers against the owned HS2 peer.
use super::*;
use tomato_core::{
    table_design::Column,
    workbench::{Cell, TableId, TableInfo},
};
fn drain(app: &mut TomatoApp) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(25);
    while !app.pending.is_empty() {
        app.logic(&ctx);
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "Hive actor timed out: {:?}",
            app.error
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    anyhow::ensure!(app.error.is_none(), "{:?}", app.error);
    for tab in &app.tabs {
        anyhow::ensure!(tab.error.is_none(), "{:?}", tab.error);
        if let Panel::Sql { workbench, .. } = &tab.panel {
            anyhow::ensure!(
                workbench.failure.is_none(),
                "Hive query/data error modal is open"
            );
        }
    }
    Ok(())
}
fn frame(app: &mut TomatoApp, id: &str, mut events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.context.clone();
    let index = app.tabs.iter().position(|t| t.id == id).unwrap();
    if let Some(modifiers) = events.iter().find_map(|e| {
        if let egui::Event::Key { modifiers, .. } = e {
            Some(*modifiers)
        } else {
            None
        }
    }) {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
    }
    let mut intent = None;
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 700.),
            )),
            time: Some(ctx.input(|i| i.time) + 0.15),
            events,
            ..Default::default()
        },
        |ui| {
            crate::table_style::sync(&ctx, app.settings.table_font(), false);
            intent = crate::panels::render(ui, &mut app.tabs[index], &mut app.icons);
        },
    );
    out.textures_delta.clear();
    if let Some(intent) = intent {
        app.handle_intent(id, intent);
    }
    out
}
fn click(app: &mut TomatoApp, id: &str, label: &str) -> anyhow::Result<()> {
    drain(app)?;
    frame(app, id, vec![]);
    drain(app)?;
    let out = frame(app, id, vec![]);
    let pos = out
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == label => {
                Some(t.pos + t.galley.size() / 2.)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("missing Hive control {label}"))?;
    frame(app, id, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            app,
            id,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
        );
    }
    Ok(())
}
fn workbench<'a>(app: &'a mut TomatoApp, id: &str) -> &'a mut crate::tabs::workbench::Workbench {
    let Panel::Sql { workbench, .. } = &mut app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
    else {
        unreachable!()
    };
    workbench
}
#[test]
#[ignore = "requires owned native Hive UI fixture"]
fn actual_hive_lazy_tree_design_preview_save_sibling_refresh_query_and_disconnect()
-> anyhow::Result<()> {
    let fixture = PathBuf::from(std::env::var("TOMATO_HIVE_UI_FIXTURE")?);
    anyhow::ensure!(
        fixture.is_absolute()
            && fixture
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("tomato-hive-ui-fixture-")),
        "requires owned profile"
    );
    let config = fixture.join("connections.json");
    anyhow::ensure!(
        Store::default_path() == config,
        "requires isolated TOMATO_DATA_DIR"
    );
    let before = std::fs::read(&config)?;
    let store = Store::load(&config)?;
    let c = store.connections[0].clone();
    anyhow::ensure!(
        c.kind() == "HIVE" && c.text("host") == "127.0.0.1",
        "loopback Hive only"
    );
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app =
        TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(config.clone()));
    app.open_connection(c.clone());
    assert!(
        app.credential.is_none(),
        "NoSASL must not request an unsaved password"
    );
    drain(&mut app)?;
    assert!(
        app.tabs.is_empty(),
        "opening Hive should create only its catalog tree"
    );
    let host = &app.sql_hosts[&c.id];
    assert!(host.direct.as_ref().unwrap().loaded);
    assert!(host.tables.is_empty());
    assert!(host.schemas.iter().any(|s| s == "未打开库"));
    app.open_direct_schema(c.clone(), "default".into(), false);
    drain(&mut app)?;
    let host = &app.sql_hosts[&c.id];
    assert!(host.tables.contains_key("default"));
    assert!(!host.tables.contains_key("未打开库"));
    let table = TableInfo {
        table: TableId {
            schema: "default".into(),
            name: "items".into(),
        },
        view: false,
    };
    app.open_table(c.clone(), table.clone(), false);
    let data = app.active.clone().unwrap();
    drain(&mut app)?;
    assert_eq!(
        workbench(&mut app, &data)
            .page
            .as_ref()
            .unwrap()
            .definition
            .columns
            .len(),
        2
    );
    app.open_table(c.clone(), table.clone(), true);
    let design = app.active.clone().unwrap();
    drain(&mut app)?;
    let draft = workbench(&mut app, &design)
        .designer
        .as_mut()
        .expect("Hive must use the real designer");
    assert_eq!(draft.request.columns.len(), 2);
    draft.request.columns.push(Column {
        name: "amount".into(),
        data_type: "DECIMAL".into(),
        length: "12".into(),
        scale: "2".into(),
        comment: "金额".into(),
        ..Default::default()
    });
    draft.origins.push(None);
    draft.dirty = true;
    draft.revision += 1;
    click(&mut app, &design, "SQL预览")?;
    assert!(
        workbench(&mut app, &design)
            .designer
            .as_ref()
            .unwrap()
            .preview
            .contains("ADD COLUMNS (`amount` DECIMAL(12,2) COMMENT '金额')")
    );
    click(&mut app, &design, "字段")?;
    click(&mut app, &design, "保存")?;
    drain(&mut app)?;
    let draft = workbench(&mut app, &design).designer.as_ref().unwrap();
    assert!(!draft.dirty);
    assert_eq!(draft.saved.as_ref().unwrap().definition.columns.len(), 3);
    let page = workbench(&mut app, &data).page.as_ref().unwrap();
    assert_eq!(page.definition.columns.len(), 3);
    assert_eq!(page.rows[0].len(), 3);
    assert_eq!(page.rows[0][2], Cell::Text("12.34".into()));
    // The existing selector displayed Hive, but its controller used to reject it.
    let query = crate::tabs::create_tab(
        Connection::new("SQLITE"),
        app.runtime.as_ref().map(|r| r.handle()),
    )?;
    let query_id = query.id.clone();
    app.tabs.push(query);
    app.active = Some(query_id.clone());
    app.choose_sql_connection(&query_id, &c.name);
    assert!(app.credential.is_none());
    drain(&mut app)?;
    assert_eq!(workbench(&mut app, &query_id).schema, "default");
    let Panel::Sql {
        connection: selected,
        ..
    } = &app.tabs.iter().find(|t| t.id == query_id).unwrap().panel
    else {
        unreachable!()
    };
    assert_eq!(
        selected.text("database"),
        c.text("database"),
        "Selecting a Hive database must retain its authentication/transport suffix"
    );
    if let Panel::Sql { sql, .. } = &mut app
        .tabs
        .iter_mut()
        .find(|t| t.id == query_id)
        .unwrap()
        .panel
    {
        // create_tab carries its connection-test seed; the real new-query action clears it.
        sql.clear();
    }
    workbench(&mut app, &query_id).completion.focus_requested = true;
    frame(&mut app, &query_id, vec![]);
    frame(
        &mut app,
        &query_id,
        vec![egui::Event::Text("SELECT 'hive input' AS value".into())],
    );
    if let Panel::Sql { sql, .. } = &app.tabs.iter().find(|t| t.id == query_id).unwrap().panel {
        assert_eq!(sql, "SELECT 'hive input' AS value");
    }
    click(&mut app, &query_id, "运行")?;
    drain(&mut app)?;
    let script = workbench(&mut app, &query_id)
        .script
        .as_ref()
        .expect("Run button must execute the Hive script");
    assert_eq!(script.fail_count, 0, "{}", script.info());
    assert_eq!(script.success_count, 1);
    assert!(matches!(
        script.mode,
        tomato_services::database::script::Mode::Run
    ));
    assert_eq!(script.row_results().count(), 1);
    assert_eq!(script.plans.len(), 1);
    assert!(script.status.is_some());
    let row_result = script.statements[0].result.expect("SELECT rowset");
    let Panel::Sql { results, .. } = &app.tabs.iter().find(|t| t.id == query_id).unwrap().panel
    else {
        unreachable!()
    };
    assert_eq!(results[row_result].value(0, 0), Some("hive input"));
    assert_eq!(std::fs::read(&config)?, before);
    app.disconnect_sql(&c.id);
    drain(&mut app)?;
    assert!(!app.sql_hosts.contains_key(&c.id));
    // Reopen and close through the actual application shutdown controller too.
    app.open_connection(c.clone());
    drain(&mut app)?;
    app.shutdown_file_transfers();
    assert_eq!(std::fs::read(&config)?, before);
    drop(app);
    std::fs::write(fixture.join("stop"), "stop")?;
    Ok(())
}
