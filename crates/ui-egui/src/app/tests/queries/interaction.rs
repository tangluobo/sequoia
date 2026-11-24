use super::*;
fn key(key: egui::Key, ctrl: bool) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers {
            ctrl,
            command: ctrl,
            ..Default::default()
        },
    }
}
#[test]
fn closing_native_query_name_prompt_releases_its_history_and_reopening_starts_clean()
-> anyhow::Result<()> {
    use crate::app::query_tree::Dialog;
    use crate::app::tests::native_bridge::Bridge;
    use egui::widgets::text_edit::TextEditState;
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let bridge = Bridge::new(&ctx);
    let id = egui::Id::new("named-query-name");
    let other = egui::Id::new("other-open-editor");
    TextEditState::default().store(&ctx, other);
    for _ in 0..3 {
        app.queries.dialog = Some(Dialog::save("owned-prompt-only".into()));
        bridge.frame(&ctx, vec![], |ctx| app.query_dialog(ctx));
        let mut state = TextEditState::load(&ctx, id).expect("native input state");
        let cursor = egui::text::CCursorRange::default();
        let mut undo = state.take_undoer();
        undo.add_undo_owned((cursor, "previous-name".repeat(1024)));
        state.set_undoer(undo);
        state.store(&ctx, id);
        bridge.close_viewport(
            &ctx,
            egui::ViewportId::from_hash_of("named-query-dialog"),
            |ctx| app.query_dialog(ctx),
        );
        assert!(app.queries.dialog.is_none());
        assert!(TextEditState::load(&ctx, id).is_none());
        assert!(TextEditState::load(&ctx, other).is_some());
    }
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
#[test]
fn query_ctrl_s_native_prompt_selection_cancel_and_save_before_close_preserve_actual_document()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let c = Connection::new("SQLITE");
    app.store.connections.push(c.clone());
    app.open_connection(c.clone());
    drain(&mut app)?;
    app.select_schema(&c.id, "main".into());
    drain(&mut app)?;
    app.query_action(Action::New {
        connection: c.id.clone(),
        namespace: "main".into(),
        path: String::new(),
    });
    let id = app.active.clone().unwrap();
    edit(&mut app, &id, "SELECT '保存';");
    let panel = |app: &mut TomatoApp, events: Vec<egui::Event>| {
        let mut intent = None;
        let index = app.tabs.iter().position(|tab| tab.id == id).unwrap();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| intent = crate::panels::render(ui, &mut app.tabs[index], &mut app.icons),
        );
        let point = output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.job.text == "SELECT '保存';"
            {
                Some(text.pos + egui::vec2(2., 8.))
            } else {
                None
            }
        });
        output.textures_delta.clear();
        (intent, point)
    };
    let point = panel(&mut app, vec![]).1.unwrap();
    for pressed in [true, false] {
        panel(
            &mut app,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    let intent = panel(&mut app, vec![key(egui::Key::S, true)])
        .0
        .expect("Ctrl+S must save a named query");
    assert!(matches!(intent, Intent::SaveQuery));
    app.handle_intent(&id, intent);
    let prompt = |app: &mut TomatoApp, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 700.),
                )),
                events,
                ..Default::default()
            },
            |ui| app.query_dialog(ui.ctx()),
        );
        output.textures_delta.clear();
    };
    prompt(&mut app, vec![]);
    let focus = ctx.memory(|memory| memory.focused()).unwrap();
    let state = egui::widgets::text_edit::TextEditState::load(&ctx, focus).unwrap();
    let range = state.cursor.char_range().unwrap();
    assert_eq!(usize::from(range.sorted_cursors()[0].index), 0);
    assert_eq!(usize::from(range.sorted_cursors()[1].index), 3);
    prompt(
        &mut app,
        vec![
            egui::Event::Text("实际保存".into()),
            key(egui::Key::Enter, false),
        ],
    );
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, focus).is_none());
    drain(&mut app)?;
    let file = directory
        .path()
        .join(document(&app, &id).1.scope.file("实际保存")?);
    assert_eq!(std::fs::read_to_string(&file)?, "SELECT '保存';");
    assert!(!document(&app, &id).1.dirty());
    app.query_action(Action::New {
        connection: c.id.clone(),
        namespace: "main".into(),
        path: String::new(),
    });
    let closing = app.active.clone().unwrap();
    edit(&mut app, &closing, "SELECT 55;");
    if let Panel::Sql { workbench, .. } = &mut app
        .tabs
        .iter_mut()
        .find(|tab| tab.id == closing)
        .unwrap()
        .panel
    {
        workbench.document.as_mut().unwrap().close_after_save = true;
    }
    app.handle_intent(&closing, Intent::SaveQuery);
    prompt(&mut app, vec![]);
    let cancel_focus = ctx.memory(|memory| memory.focused()).unwrap();
    prompt(&mut app, vec![key(egui::Key::Escape, false)]);
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, cancel_focus).is_none());
    assert!(!document(&app, &closing).1.close_after_save);
    assert!(document(&app, &closing).1.dirty());
    if let Panel::Sql { workbench, .. } = &mut app
        .tabs
        .iter_mut()
        .find(|tab| tab.id == closing)
        .unwrap()
        .panel
    {
        workbench.document.as_mut().unwrap().close_after_save = true;
    }
    app.handle_intent(&closing, Intent::SaveQuery);
    prompt(&mut app, vec![]);
    prompt(
        &mut app,
        vec![
            egui::Event::Text("关闭前保存".into()),
            key(egui::Key::Enter, false),
        ],
    );
    drain(&mut app)?;
    assert!(app.tabs.iter().all(|tab| tab.id != closing));
    let scope = Scope {
        connection: c.name,
        database: "main".into(),
        path: String::new(),
    };
    assert_eq!(
        std::fs::read_to_string(directory.path().join(scope.file("关闭前保存")?))?,
        "SELECT 55;"
    );
    Ok(())
}
