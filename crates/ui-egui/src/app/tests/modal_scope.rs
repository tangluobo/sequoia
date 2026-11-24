use super::*;
use native_bridge::Bridge;

#[test]
fn visible_hosts_and_redis_dialogs_block_navigation_but_hidden_owners_do_not() -> anyhow::Result<()>
{
    for hosts in [false, true] {
        let directory = tempfile::tempdir()?;
        let mut app = app(directory.path())?;
        let bridge = Bridge::new(&app.context);
        if hosts {
            app.section = 1;
            app.open_tool("hosts_file", "Hosts 文件");
            let Panel::Tool(tool) = &mut app.tabs[0].panel else {
                unreachable!()
            };
            let state = tool.hosts.get_or_insert_with(Default::default);
            state.paths = Some((
                directory.path().join("groups.json"),
                directory.path().join("hosts"),
            ));
            state.accept(tomato_services::hosts::Snapshot {
                config: Default::default(),
                system: "127.0.0.1 localhost".into(),
                config_stamp: None,
                system_stamp: None,
            });
            state.rename = Some("fixture".into());
        } else {
            app.section = 0;
            let mut tab = tabs::create_tab(Connection::new("REDIS"), None)?;
            let Panel::Redis { view, .. } = &mut tab.panel else {
                unreachable!()
            };
            view.loaded = true;
            view.dialog = Some(crate::tabs::redis::Dialog::Delete {
                keys: vec!["fixture".into()],
            });
            app.active = Some(tab.id.clone());
            app.tabs.push(tab);
        }
        let section = app.section;
        assert!(app.root_modal_open());
        settings_click(&mut app, &bridge);
        assert_eq!(
            app.section, section,
            "a visible dialog must block settings navigation"
        );
        app.section = 4;
        assert!(
            !app.root_modal_open(),
            "a hidden owner must not lock another section"
        );
        assert!(!directory.path().join("connections.json").exists());
    }
    Ok(())
}

fn app(directory: &std::path::Path) -> anyhow::Result<TomatoApp> {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    Ok(TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.join("connections.json")),
    ))
}

fn settings_click(app: &mut TomatoApp, bridge: &Bridge) {
    let ctx = app.context.clone();
    bridge.root_frame(&ctx, vec![], vec![], |ui| app.ui(ui));
    let point = egui::pos2(
        app.layout.header.right() - 111.,
        app.layout.header.top() + 13.,
    );
    for pressed in [true, false] {
        bridge.root_frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            vec![],
            |ui| app.ui(ui),
        );
    }
}

#[test]
fn background_sql_notices_do_not_lock_the_visible_workspace() -> anyhow::Result<()> {
    for designer in [false, true] {
        let directory = tempfile::tempdir()?;
        let mut app = app(directory.path())?;
        let bridge = Bridge::new(&app.context);
        let mut hidden = tabs::create_tab(Connection::new("MYSQL"), None)?;
        let hidden_id = hidden.id.clone();
        let Panel::Sql { workbench, .. } = &mut hidden.panel else {
            unreachable!()
        };
        if designer {
            let mut draft = crate::tabs::table_design::Draft::new("MYSQL", "owned".into())?;
            draft.notice = Some(crate::tabs::table_design::Notice {
                title: "表结构".into(),
                header: Some("操作失败".into()),
                text: "isolated background failure".into(),
                error: true,
                focus: true,
            });
            workbench.designer = Some(Box::new(draft));
        } else {
            workbench.browser = Some(Box::new(crate::tabs::database_objects::window::Draft {
                notice: Some("isolated background failure".into()),
                ..Default::default()
            }));
        }
        let visible = tabs::create_tab(Connection::new("MYSQL"), None)?;
        app.active = Some(visible.id.clone());
        app.tabs.extend([hidden, visible]);
        settings_click(&mut app, &bridge);
        assert_eq!(
            app.section, 4,
            "background notice must not disable settings"
        );

        // Visiting the owner must still show its original native notice and
        // block the root until the notice is dismissed.
        app.section = 0;
        app.active = Some(hidden_id);
        settings_click(&mut app, &bridge);
        assert_eq!(app.section, 0, "visible notice must remain modal");
        bridge.text("确定")?;
        let ctx = app.context.clone();
        bridge.root_frame(
            &ctx,
            vec![],
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            |ui| app.ui(ui),
        );
        let Panel::Sql { workbench, .. } = &app.tabs[0].panel else {
            unreachable!()
        };
        assert!(if designer {
            workbench.designer.as_ref().unwrap().notice.is_none()
        } else {
            workbench.browser.as_ref().unwrap().notice.is_none()
        });
        settings_click(&mut app, &bridge);
        assert_eq!(app.section, 4);
        assert!(!directory.path().join("connections.json").exists());
    }
    Ok(())
}

#[test]
fn hidden_tool_editor_does_not_lock_a_different_module() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut app = app(directory.path())?;
    let bridge = Bridge::new(&app.context);
    app.section = 1;
    app.open_tool("port_mapping", "端口映射");
    let Panel::Tool(tool) = &mut app.tabs[0].panel else {
        unreachable!()
    };
    tool.port_mapping = Some(Box::new(crate::tabs::port_mapping::State::new(
        directory.path().join("port-mappings.json"),
        None,
        directory.path().join("known_hosts"),
    )));
    tool.port_mapping.as_mut().unwrap().editor = Some(crate::tabs::port_mapping::Editor {
        mapping: Default::default(),
        local: String::new(),
        remote: String::new(),
        editing: None,
        focus: true,
    });
    assert!(app.root_modal_open());
    // An outstanding editor belongs to the tool, whose panel is absent in settings.
    app.section = 4;
    // Keep the active tool id just as module switching does for a hidden panel.
    assert!(!app.root_modal_open());
    let ctx = app.context.clone();
    bridge.root_frame(&ctx, vec![], vec![], |ui| app.ui(ui));
    let point = egui::pos2(app.layout.nav.center().x, app.layout.nav.top() + 40.);
    for pressed in [true, false] {
        bridge.root_frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            vec![],
            |ui| app.ui(ui),
        );
    }
    assert_eq!(app.section, 0);
    Ok(())
}

#[test]
fn globally_rendered_result_dialog_remains_modal_for_a_background_tab() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut app = app(directory.path())?;
    let bridge = Bridge::new(&app.context);
    let mut hidden = tabs::create_tab(Connection::new("MYSQL"), None)?;
    let Panel::Sql {
        results, workbench, ..
    } = &mut hidden.panel
    else {
        unreachable!()
    };
    results.push(tomato_services::QueryResult {
        columns: vec!["id".into()],
        rows: vec![vec!["1".into()]],
        ..Default::default()
    });
    let mut state = crate::tabs::query_results::State::default();
    state.initialize(&results[0]);
    state.view.dialog = Some(crate::tabs::query_results::view::Dialog {
        focus: true,
        column: 0,
        input: String::new(),
        editor: Default::default(),
    });
    workbench.query_results.push(state);
    let visible = tabs::create_tab(Connection::new("MYSQL"), None)?;
    app.active = Some(visible.id.clone());
    app.tabs.extend([hidden, visible]);
    settings_click(&mut app, &bridge);
    assert_eq!(app.section, 0);
    bridge.text("取消")?;
    let ctx = app.context.clone();
    bridge.root_frame(
        &ctx,
        vec![],
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        |ui| app.ui(ui),
    );
    let Panel::Sql { workbench, .. } = &app.tabs[0].panel else {
        unreachable!()
    };
    assert!(workbench.query_results[0].view.dialog.is_none());
    settings_click(&mut app, &bridge);
    assert_eq!(app.section, 4);
    Ok(())
}
