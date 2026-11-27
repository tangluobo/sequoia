use super::*;
mod ai_dialog;
mod ai_sql;
mod batch_edit;
mod clipboard_shortcuts;
mod cloud_mq;
mod designer;
mod game_dungeons;
pub(crate) mod header_divider;
mod hive;
mod layout_alignment;
mod markdown_alignment;
mod modal_scope;
pub(crate) mod native_bridge;
mod native_messages;
mod object_tree;
mod objects;
mod persistence;
mod queries;
mod query_rows;
mod recovery;
mod sql_completion;
mod sql_icons;
mod sql_scope;
mod sql_tree;
mod table_tabs;
mod tool_migration;
mod trae;
#[cfg(windows)]
mod window_focus;
#[test]
fn content_undo_skips_caret_moves_redoes_after_navigation_and_lazily_retains_unchanged_sql() {
    use egui::text::{CCursor, CCursorRange};
    let cursor = |index| CCursorRange::one(CCursor::new(index));
    let mut undo = egui::widgets::text_edit::TextEditState::default().undoer();
    undo.add_undo(&(cursor(0), "old SQL".to_owned()));
    undo.add_undo(&(cursor(7), "new SQL".to_owned()));
    let old = undo
        .undo_by(&(cursor(2), "new SQL".to_owned()), |left, right| {
            left.1 == right.1
        })
        .unwrap();
    assert_eq!(old.1, "old SQL");
    let new = undo
        .redo_by(&(cursor(3), "old SQL".to_owned()), |left, right| {
            left.1 == right.1
        })
        .unwrap();
    assert_eq!(new.1, "new SQL");
    for time in 1..100 {
        undo.feed_state_lazy(
            f64::from(time),
            |saved| saved.1 == "new SQL",
            || panic!("unchanged content must not allocate another SQL snapshot"),
        );
    }
    undo.feed_state_lazy(
        101.,
        |saved| saved.1 == "typed SQL",
        || (cursor(9), "typed SQL".into()),
    );
    undo.feed_state_lazy(
        103.,
        |saved| saved.1 == "typed SQL",
        || (cursor(9), "typed SQL".into()),
    );
    assert_eq!(
        undo.undo_by(&(cursor(1), "typed SQL".into()), |left, right| left.1
            == right.1)
            .unwrap()
            .1,
        "new SQL"
    );
}
#[test]
fn undo_snapshot_budget_keeps_recent_edits_and_releases_large_histories() {
    use egui::{
        text::{CCursor, CCursorRange},
        widgets::text_edit::TextEditState,
    };
    let cursor = CCursorRange::one(CCursor::new(0));
    let mut undo = TextEditState::default().undoer();
    for index in 0..40 {
        undo.add_undo(&(cursor, format!("{index:02}{}", "x".repeat(128 * 1024))));
    }
    let mut state = TextEditState::default();
    state.set_undoer(undo);
    assert!(state.trim_undo_memory(512 * 1024) <= 512 * 1024);
    let mut undo = state.undoer();
    let current = (cursor, format!("39{}", "x".repeat(128 * 1024)));
    assert!(undo.undo(&current).unwrap().1.starts_with("38"));
    state.set_undoer(undo);
    assert!(state.trim_undo_memory(512 * 1024) <= 512 * 1024);
    let mut undo = state.undoer();
    let previous = (cursor, format!("38{}", "x".repeat(128 * 1024)));
    assert!(undo.redo(&previous).unwrap().1.starts_with("39"));
    state.set_undoer(undo);
    assert_eq!(state.trim_undo_memory(0), 0);
    assert!(!state.undoer().has_undo(&current));
}
#[test]
fn closing_markdown_releases_egui_undo_state_without_clearing_other_editors() -> anyhow::Result<()>
{
    let temporary = tempfile::tempdir()?;
    let first = temporary.path().join("first.md");
    let second = temporary.path().join("second.md");
    std::fs::write(&first, "first")?;
    std::fs::write(&second, "second")?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(temporary.path().join("connections.json")),
    );
    app.open_editor(first);
    drain(&mut app)?;
    assert!(render_frame(&ctx, &mut app));
    let tab = app.tabs[0].id.clone();
    let Panel::Editor { markdown, .. } = &mut app.tabs[0].panel else {
        panic!("editor")
    };
    let first_id = markdown.editor_id.unwrap();
    let query_id = egui::Id::new("closed-find-query");
    markdown.find_field_ids[0] = Some(query_id);
    egui::widgets::text_edit::TextEditState::default().store(&ctx, query_id);
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, first_id).is_some());
    app.open_editor(second);
    drain(&mut app)?;
    assert!(render_frame(&ctx, &mut app));
    let Panel::Editor { markdown, .. } = &app.tabs[1].panel else {
        panic!("editor")
    };
    let second_id = markdown.editor_id.unwrap();
    app.close(&tab);
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, first_id).is_none());
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, query_id).is_none());
    assert!(egui::widgets::text_edit::TextEditState::load(&ctx, second_id).is_some());
    Ok(())
}
#[test]
fn connection_drag_save_conflicts_keep_the_old_model_and_file_move_updates_open_editor()
-> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("connections.json");
    let mut group = Connection::new("SSH");
    group.kind = None;
    let mut connection = Connection::new("LOCAL_DIRECTORY");
    connection.set(
        "localDirectoryPath",
        temporary.path().to_string_lossy().as_ref(),
    );
    let store = Store {
        connections: vec![group.clone(), connection.clone()],
        ..Default::default()
    };
    store.save(&path)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &egui::Context::default(),
        runtime,
        ConnectionFileRepository::new(path.clone()),
    );
    app.apply_tree_drop(tree_drag::Action::Connection {
        id: connection.id.clone(),
        target: tomato_core::tree::DropTarget::Into(group.id.clone()),
    });
    assert!(app.error.is_none());
    assert_eq!(
        Store::load(&path)?
            .connections
            .iter()
            .find(|node| node.id == connection.id)
            .unwrap()
            .parent_id
            .as_deref(),
        Some(group.id.as_str())
    );
    std::fs::write(&path, "external change")?;
    app.apply_tree_drop(tree_drag::Action::Connection {
        id: connection.id.clone(),
        target: tomato_core::tree::DropTarget::Root,
    });
    assert!(app.error.is_some());
    assert_eq!(
        app.store
            .connections
            .iter()
            .find(|node| node.id == connection.id)
            .unwrap()
            .parent_id
            .as_deref(),
        Some(group.id.as_str())
    );
    assert_eq!(std::fs::read_to_string(&path)?, "external change");
    let file = temporary.path().join("移动.md");
    let parent = temporary.path().join("目标");
    std::fs::write(&file, "draft")?;
    std::fs::create_dir(&parent)?;
    app.error = None;
    app.open_editor(file.clone());
    drain(&mut app)?;
    app.apply_tree_drop(tree_drag::Action::File {
        connection,
        path: file,
        parent: parent.clone(),
    });
    drain(&mut app)?;
    assert!(app.error.is_none());
    assert!(
        matches!(&app.tabs[0].panel, Panel::Editor { path, text, .. } if path == &parent.join("移动.md") && text == "draft")
    );
    Ok(())
}
#[test]
fn json_actions_restore_history_and_report_errors_in_the_original_output_and_footer()
-> anyhow::Result<()> {
    use tomato_services::json_history::Action;
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    app.open_tool("json_tool", "JSON处理工具");
    app.section = 1;
    let id = app.active.clone().expect("JSON tab");
    drain(&mut app)?;
    let path = directory.path().join("json_history.json");
    app.handle_intent(
        &id,
        Intent::Command(Box::new(Command::JsonHistory {
            path: path.clone(),
            action: Action::Format {
                json: "{\"ok\":true}".into(),
            },
        })),
    );
    drain(&mut app)?;
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("JSON tool")
    };
    assert!(tool.json_tree);
    assert_eq!(tool.json_status.text, "格式化成功！");
    let key = tool.history[0].key.clone();
    let saved = std::fs::read(&path)?;
    app.handle_intent(
        &id,
        Intent::Command(Box::new(Command::JsonHistory {
            path: path.clone(),
            action: Action::Restore(key),
        })),
    );
    drain(&mut app)?;
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("JSON tool")
    };
    assert_eq!(tool.json_status.text, "已从历史恢复！");
    assert!(tool.input.contains("\"ok\""));
    app.handle_intent(
        &id,
        Intent::Command(Box::new(Command::JsonHistory {
            path: path.clone(),
            action: Action::Format { json: "[".into() },
        })),
    );
    drain(&mut app)?;
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("JSON tool")
    };
    assert!(tool.json_status.error && tool.json_status.until.is_none());
    assert!(tool.output.starts_with("JSON 格式错误:"));
    assert!(!tool.json_tree);
    assert_eq!(std::fs::read(&path)?, saved);
    assert!(render_frame(&ctx, &mut app));
    app.handle_intent(
        &id,
        Intent::Command(Box::new(Command::Json {
            action: "minify".into(),
            text: "{\"ok\": true}".into(),
        })),
    );
    drain(&mut app)?;
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("JSON tool")
    };
    assert_eq!(tool.output, "{\"ok\":true}");
    assert_eq!(tool.json_status.text, "压缩成功！");
    assert!(!tool.json_status.error && tool.json_status.until.is_some());
    app.close(&id);
    let mut saved_tool = Connection::new("TOOL");
    saved_tool.set("toolType", "json_tool");
    app.open_connection(saved_tool);
    app.section = 0;
    drain(&mut app)?;
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("saved JSON tool")
    };
    assert_eq!(tool.history_path, path);
    assert_eq!(tool.history.len(), 1);
    Ok(())
}
#[test]
fn failed_server_save_keeps_dirty_draft_and_prevents_closing_the_tab() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(&path);
    let mut store = repository.load()?;
    let connection = Connection::new("HTTP_SERVER");
    store.connections.push(connection.clone());
    repository.save(&store)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&egui::Context::default(), runtime, repository);
    app.open_connection(connection);
    let id = app.tabs[0].id.clone();
    let Panel::Server(view) = &mut app.tabs[0].panel else {
        panic!("server tab")
    };
    view.config.bind_address = "127.0.0.1".into();
    view.changed = true;
    let config = view.config.clone();
    std::fs::write(&path, "external edit")?;
    app.handle_intent(&id, Intent::ConfigureServer(config));
    let Panel::Server(view) = &app.tabs[0].panel else {
        panic!("server tab")
    };
    assert!(view.changed && view.save_failed);
    assert!(app.error.is_some());
    app.close(&id);
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(std::fs::read_to_string(path)?, "external edit");
    Ok(())
}
#[test]
fn saved_connection_groups_start_collapsed_and_search_reveals_matching_descendants()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(directory.path().join("connections.json"));
    let mut store = repository.load()?;
    let mut group = Connection::new("TOOL");
    group.kind = None;
    group.name = "原版分组".into();
    let mut child = Connection::new("LOCAL_DIRECTORY");
    child.name = "匹配目录".into();
    child.parent_id = Some(group.id.clone());
    store.connections = vec![child.clone(), group.clone()];
    repository.save(&store)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let app = TomatoApp::with_repository(&egui::Context::default(), runtime, repository);
    let collapsed = db_tree::rows(
        &app.store,
        &app.tabs,
        &app.collapsed,
        db_tree::Hosts {
            remote: &app.remote_hosts,
            backups: &app.backups,
            sql: &app.sql_hosts,
            queries: &app.queries,
            ssh: &app.ssh_hosts,
            local: &app.local_trees,
            mongo: &app.mongo_hosts,
            redis: &app.redis_hosts,
        },
        "",
    );
    assert_eq!(collapsed.len(), 1);
    assert_eq!(collapsed[0].key(), group.id);
    let searched = db_tree::rows(
        &app.store,
        &app.tabs,
        &app.collapsed,
        db_tree::Hosts {
            remote: &app.remote_hosts,
            backups: &app.backups,
            sql: &app.sql_hosts,
            queries: &app.queries,
            ssh: &app.ssh_hosts,
            local: &app.local_trees,
            mongo: &app.mongo_hosts,
            redis: &app.redis_hosts,
        },
        "匹配",
    );
    assert!(searched.iter().any(|row| row.key() == child.id));
    Ok(())
}

#[test]
fn welcome_tools_and_connection_dialog_render_without_native_window() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("workspace.json")),
    );
    assert!(render_frame(&ctx, &mut app));
    assert!(
        (app.layout.nav.width() - 62.0).abs() < 0.1,
        "nav {:?}, sidebar {:?}, header {:?}",
        app.layout.nav,
        app.layout.sidebar,
        app.layout.header
    );
    assert!((app.layout.sidebar.width() - 240.0).abs() < 0.1);
    assert!((app.layout.header.height() - 52.0).abs() < 0.1);
    assert!((app.layout.header.left() - 304.0).abs() < 0.1);
    assert!(app.tabs.is_empty());
    for &(kind, label) in TOOLS {
        app.open_tool(kind, label);
        assert!(render_frame(&ctx, &mut app), "tool {kind}");
    }
    app.edit_connection(Connection::new("MYSQL"));
    assert!(render_frame(&ctx, &mut app));
    assert!(!directory.path().join("workspace.json").exists());
    Ok(())
}

#[test]
fn closing_a_tab_discards_its_pending_results() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("workspace.json")),
    );
    app.open_tool("json_tool", "JSON");
    let id = app
        .active
        .clone()
        .ok_or_else(|| anyhow::anyhow!("missing tool tab"))?;
    app.submit(
        &id,
        Command::Json {
            text: "{}".into(),
            action: "format".into(),
        },
        Purpose::Normal,
    );
    app.close(&id);
    assert!(app.pending.is_empty());
    assert!(app.tabs.is_empty());
    Ok(())
}

fn render_frame(ctx: &egui::Context, app: &mut TomatoApp) -> bool {
    let mut output = render_frame_with_events(ctx, app, vec![]);
    let painted = !output.shapes.is_empty();
    // Headless test intentionally has no GPU backend to apply texture uploads.
    output.textures_delta.clear();
    painted
}

fn render_frame_with_events(
    ctx: &egui::Context,
    app: &mut TomatoApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1280.0, 800.0),
        )),
        events,
        ..Default::default()
    };
    let mut output = ctx.run_ui(input, |ui| app.ui(ui));
    output.textures_delta.clear();
    output
}

fn click_app(
    ctx: &egui::Context,
    app: &mut TomatoApp,
    position: egui::Pos2,
) -> egui::FullOutput {
    let mut output = render_frame_with_events(
        ctx,
        app,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    output.textures_delta.clear();
    output = render_frame_with_events(
        ctx,
        app,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    output.textures_delta.clear();
    output
}

fn text_position(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text_shape) if text_shape.galley.job.text == text => {
            Some(text_shape.pos + text_shape.galley.size() / 2.0)
        }
        _ => None,
    })
}

#[test]
fn root_connection_create_button_works_when_the_tree_is_full() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    let connections = (0..80)
        .map(|index| {
            let mut connection = Connection::new("SSH");
            connection.name = format!("主机 {index}");
            connection
        })
        .collect();
    Store {
        connections,
        ..Default::default()
    }
    .save(&path)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
    let output = render_frame_with_events(&ctx, &mut app, vec![]);
    let plus = text_position(&output, "+").expect("root connection create button");
    let output = click_app(&ctx, &mut app, plus);
    assert_eq!(app.picker, Some(0));
    assert!(!output.shapes.is_empty());
    Ok(())
}

#[test]
fn database_tree_table_editor_and_shared_session_lifecycle() -> anyhow::Result<()> {
    use tomato_core::workbench::*;
    let directory = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let ctx = egui::Context::default();
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let c = Connection::new("SQLITE");
    app.store.connections.push(c.clone());
    app.open_connection(c.clone());
    drain(&mut app)?;
    let root = app.active.clone().expect("root tab");
    app.submit(&root, Command::Sql { connection: c.clone(), sql: "CREATE TABLE sample(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO sample VALUES(1,'中文');".into() }, Purpose::Normal);
    drain(&mut app)?;
    app.select_schema(&c.id, "main".into());
    drain(&mut app)?;
    let table = TableInfo {
        table: TableId {
            schema: "main".into(),
            name: "sample".into(),
        },
        view: false,
    };
    let rows = db_tree::rows(
        &app.store,
        &app.tabs,
        &app.collapsed,
        db_tree::Hosts {
            remote: &app.remote_hosts,
            backups: &app.backups,
            sql: &app.sql_hosts,
            queries: &app.queries,
            ssh: &app.ssh_hosts,
            local: &app.local_trees,
            mongo: &app.mongo_hosts,
            redis: &app.redis_hosts,
        },
        "",
    );
    assert!(
        rows.iter()
            .any(|row| matches!(row.node, db_tree::Node::Table(_, t) if t.table == table.table))
    );
    app.open_table(c.clone(), table, false);
    drain(&mut app)?;
    let id = app.active.clone().expect("table tab");
    assert!(render_frame(&ctx, &mut app));
    let tab = app.tabs.iter_mut().find(|tab| tab.id == id).expect("table");
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        panic!("SQL table")
    };
    assert_eq!(
        workbench.page.as_ref().expect("page").rows[0][1],
        Cell::Text("中文".into())
    );
    workbench.structure = true;
    assert!(render_frame(&ctx, &mut app));
    app.submit(
        &root,
        Command::Database {
            connection: c.clone(),
            action: DatabaseAction::Transaction {
                action: tomato_core::workbench::TransactionAction::Begin,
            },
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    app.close(&root);
    assert!(
        db_tree::rows(
            &app.store,
            &app.tabs,
            &app.collapsed,
            db_tree::Hosts {
                remote: &app.remote_hosts,
                backups: &app.backups,
                sql: &app.sql_hosts,
                queries: &app.queries,
                ssh: &app.ssh_hosts,
                local: &app.local_trees,
                mongo: &app.mongo_hosts,
                redis: &app.redis_hosts
            },
            ""
        )
        .iter()
        .any(
            |row| matches!(row.node,db_tree::Node::Table(_,table) if table.table.name == "sample")
        )
    );
    app.open_connection(c.clone());
    let reopened = app.active.clone().expect("reopened query");
    let tab = app
        .tabs
        .iter()
        .find(|tab| tab.id == reopened)
        .expect("query");
    assert!(
        matches!(&tab.panel,Panel::Sql {workbench,..} if workbench.transaction && workbench.schema == "main")
    );
    assert_eq!(app.tabs.iter().filter(|tab| matches!(&tab.panel,Panel::Sql {workbench,..} if !workbench.schemas.is_empty())).count(),1);
    app.submit(
        &reopened,
        Command::Database {
            connection: c.clone(),
            action: DatabaseAction::Transaction {
                action: tomato_core::workbench::TransactionAction::Commit,
            },
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    app.close(&reopened);
    // Closing and reopening the query tab must preserve the live in-memory database.
    app.submit(
        &id,
        Command::Database {
            connection: c,
            action: DatabaseAction::Page {
                request: PageRequest {
                    table: TableId {
                        schema: "main".into(),
                        name: "sample".into(),
                    },
                    offset: 0,
                    limit: 1000,
                    sort: None,
                    query: Default::default(),
                },
            },
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    assert!(
        app.tabs
            .iter()
            .find(|tab| tab.id == id)
            .expect("table")
            .error
            .is_none()
    );
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}

fn drain(app: &mut TomatoApp) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.pending.is_empty() {
        app.logic(&app.context.clone());
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "background command timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    for tab in &app.tabs {
        anyhow::ensure!(tab.error.is_none(), "background error: {:?}", tab.error);
    }
    Ok(())
}

#[test]
fn local_directory_tree_loads_lazily_refreshes_children_and_reuses_saved_editor_tabs()
-> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = temporary.path().join("files");
    let folder = root.join("中文目录");
    std::fs::create_dir_all(&folder)?;
    std::fs::write(root.join("README.MD"), "# 原文\n")?;
    std::fs::write(root.join("z.txt"), "text")?;
    std::fs::write(folder.join("nested.md"), "# nested")?;
    let repository = ConnectionFileRepository::new(temporary.path().join("connections.json"));
    let mut connection = Connection::new("LOCAL_DIRECTORY");
    connection.name = "本地文档".into();
    connection.set("localDirectoryPath", root.to_string_lossy().as_ref());
    let mut store = repository.load()?;
    store.connections.push(connection.clone());
    repository.save(&store)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&egui::Context::default(), runtime, repository);
    app.open_connection(connection.clone());
    assert!(
        app.tabs.is_empty(),
        "a directory opens its tree, not a substitute browser tab"
    );
    drain(&mut app)?;
    let rows = db_tree::rows(
        &app.store,
        &app.tabs,
        &app.collapsed,
        db_tree::Hosts {
            remote: &app.remote_hosts,
            backups: &app.backups,
            sql: &app.sql_hosts,
            queries: &app.queries,
            ssh: &app.ssh_hosts,
            local: &app.local_trees,
            mongo: &app.mongo_hosts,
            redis: &app.redis_hosts,
        },
        "",
    );
    assert_eq!(
        rows.iter().map(db_tree::Row::name).collect::<Vec<_>>(),
        ["本地文档", "中文目录", "README.MD", "z.txt"]
    );
    assert_eq!(rows[1].icon(), "images/connect/folder.png");
    assert_eq!(rows[2].icon(), "images/connect/md.png");
    assert_eq!(rows[3].icon(), "images/connect/code.png");
    assert!(
        !app.local_trees[&connection.id]
            .directories
            .contains_key(&folder)
    );
    app.local_tree_open(&connection, Some(folder.clone()), false);
    drain(&mut app)?;
    std::fs::write(folder.join("second.md"), "# second")?;
    app.local_tree_open(&connection, Some(folder.clone()), true);
    drain(&mut app)?;
    assert!(
        app.local_trees[&connection.id]
            .directories
            .contains_key(&root)
    );
    assert_eq!(
        app.local_trees[&connection.id].directories[&folder].len(),
        2
    );
    app.collapsed.insert(connection.id.clone());
    app.collapsed
        .insert(local_tree::key(&connection.id, &folder));
    let rows = db_tree::rows(
        &app.store,
        &app.tabs,
        &app.collapsed,
        db_tree::Hosts {
            remote: &app.remote_hosts,
            backups: &app.backups,
            sql: &app.sql_hosts,
            queries: &app.queries,
            ssh: &app.ssh_hosts,
            local: &app.local_trees,
            mongo: &app.mongo_hosts,
            redis: &app.redis_hosts,
        },
        "second",
    );
    assert_eq!(
        rows.iter().map(db_tree::Row::name).collect::<Vec<_>>(),
        ["本地文档", "中文目录", "second.md"]
    );
    let markdown = root.join("README.MD");
    app.open_editor(markdown.clone());
    drain(&mut app)?;
    let id = app.active.clone().expect("editor");
    let Panel::Editor { text, .. } = &app.tabs[0].panel else {
        panic!("editor")
    };
    assert_eq!(text, "# 原文\n");
    app.open_editor(markdown.clone());
    assert_eq!(app.tabs.len(), 1);
    if let Panel::Editor { text, .. } = &mut app.tabs[0].panel {
        *text = "# 保存\n".into();
    }
    app.handle_intent(&id, Intent::Save(markdown.clone(), "# 保存\n".into()));
    drain(&mut app)?;
    assert_eq!(std::fs::read_to_string(&markdown)?, "# 保存\n");
    assert!(!app.tabs[0].dirty());
    app.store.connections.clear();
    app.logic(&app.context.clone());
    assert!(
        app.local_trees.is_empty(),
        "deleting a connection releases its tree"
    );
    Ok(())
}
#[test]
fn refreshing_a_directory_ignores_stale_results_and_reports_io_errors() -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(temporary.path().join("connections.json"));
    let mut c = Connection::new("LOCAL_DIRECTORY");
    c.set(
        "localDirectoryPath",
        temporary.path().join("missing").to_string_lossy().as_ref(),
    );
    let mut store = repository.load()?;
    store.connections.push(c.clone());
    repository.save(&store)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&egui::Context::default(), runtime, repository);
    app.open_connection(c.clone());
    let generation = app
        .pending
        .values()
        .find_map(|pending| match pending.purpose {
            Purpose::LocalTree { generation, .. } => Some(generation),
            _ => None,
        })
        .expect("request");
    app.local_tree_open(&c, None, true);
    app.local_tree_reply(
        &c.id,
        std::path::Path::new(c.text("localDirectoryPath")),
        generation,
        Ok(Output::Files(Vec::new())),
    );
    assert!(app.local_trees[&c.id].directories.is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.pending.is_empty() {
        app.logic(&app.context.clone());
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "directory request timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        app.error
            .as_ref()
            .is_some_and(|message| message.starts_with("加载目录失败"))
    );
    Ok(())
}

#[test]
fn tree_file_operations_refresh_open_rename_and_close_real_editor_tabs() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(root.path().join("connections.json"));
    let directory = root.path().join("docs");
    std::fs::create_dir(&directory)?;
    let mut connection = Connection::new("LOCAL_DIRECTORY");
    connection.set("localDirectoryPath", directory.to_string_lossy().as_ref());
    let mut store = repository.load()?;
    store.connections.push(connection.clone());
    repository.save(&store)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&egui::Context::default(), runtime, repository);
    app.open_connection(connection.clone());
    drain(&mut app)?;
    app.local_edit_start(
        connection.clone(),
        directory.clone(),
        local_tree::Kind::Markdown,
    );
    app.local_edit.as_mut().unwrap().name = "原文".into();
    app.local_edit_submit();
    drain(&mut app)?;
    assert!(directory.join("原文.md").exists());
    assert_eq!(app.tabs.len(), 1);
    let id = app.tabs[0].id.clone();
    app.handle_intent(
        &id,
        Intent::Save(directory.join("原文.md"), "# 保存内容".into()),
    );
    drain(&mut app)?;
    app.local_edit_start(
        connection.clone(),
        directory.join("原文.md"),
        local_tree::Kind::Rename,
    );
    app.local_edit.as_mut().unwrap().name = "改名.md".into();
    app.local_edit_submit();
    drain(&mut app)?;
    assert!(
        matches!(&app.tabs[0].panel,Panel::Editor{path,..} if path==&directory.join("改名.md"))
    );
    app.open_editor(directory.join("改名.md"));
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(
        std::fs::read_to_string(directory.join("改名.md"))?,
        "# 保存内容"
    );
    app.local_selection
        .insert((connection.id.clone(), directory.join("改名.md")));
    app.local_edit_start(
        connection,
        directory.join("改名.md"),
        local_tree::Kind::Delete,
    );
    app.local_edit_submit();
    drain(&mut app)?;
    assert!(!directory.join("改名.md").exists());
    assert!(app.tabs.is_empty());
    assert!(app.local_edit.is_none());
    Ok(())
}

#[test]
fn save_before_close_waits_for_success_and_keeps_failed_edits_open() -> anyhow::Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("document.md");
    std::fs::write(&path, "original")?;
    let repository = ConnectionFileRepository::new(root.path().join("connections.json"));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&egui::Context::default(), runtime, repository);
    app.open_editor(path.clone());
    drain(&mut app)?;
    let id = app.tabs[0].id.clone();
    if let Panel::Editor { text, markdown, .. } = &mut app.tabs[0].panel {
        *text = "saved before close".into();
        markdown.close_after_save = true;
    }
    app.handle_intent(&id, Intent::Save(path.clone(), "saved before close".into()));
    assert_eq!(app.tabs.len(), 1);
    drain(&mut app)?;
    assert!(app.tabs.is_empty());
    assert_eq!(std::fs::read_to_string(&path)?, "saved before close");
    app.open_editor(path.clone());
    drain(&mut app)?;
    let id = app.tabs[0].id.clone();
    std::fs::remove_file(&path)?;
    std::fs::create_dir(&path)?;
    if let Panel::Editor { text, markdown, .. } = &mut app.tabs[0].panel {
        *text = "keep failed edits".into();
        markdown.close_after_save = true;
    }
    app.handle_intent(&id, Intent::Save(path, "keep failed edits".into()));
    assert!(drain(&mut app).is_err());
    assert_eq!(app.tabs.len(), 1);
    assert!(
        matches!(&app.tabs[0].panel,Panel::Editor{text,markdown,..} if text=="keep failed edits"&&!markdown.close_after_save)
    );
    Ok(())
}
mod development;
