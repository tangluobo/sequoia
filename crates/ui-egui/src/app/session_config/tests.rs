use super::*;
use crate::app::tests::native_bridge::Bridge;
struct Fixture {
    app: TomatoApp,
    ctx: egui::Context,
    path: PathBuf,
    _dir: tempfile::TempDir,
}
impl Fixture {
    fn new() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("connections.json");
        let mut connection = Connection::new("LOCAL_TERMINAL");
        connection.set("terminalType", "cmd");
        connection.name = "终端设置验收".into();
        connection.set("future", serde_json::json!({"nested":[1,2,3]}));
        connection.set("rdpOpenMode", "leave-untouched");
        Store {
            connections: vec![connection.clone()],
            ..Default::default()
        }
        .save(&path)?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        let ctx = egui::Context::default();
        let mut app =
            TomatoApp::with_repository(&ctx, runtime, ConnectionFileRepository::new(&path));
        // Configuration UI does not need a console process. Use an owned local
        // file transport under the terminal view; real SSH is covered natively.
        let mut transport = connection.clone();
        transport.kind = Some("LOCAL_DIRECTORY".into());
        transport.set("localDirectoryPath", dir.path().to_string_lossy().as_ref());
        let mut tab = tabs::create_tab(transport, Some(app.runtime.as_ref().unwrap().handle()))?;
        if let Panel::Terminal {
            connection: target,
            view,
            ..
        } = &mut tab.panel
        {
            *target = connection;
            view.ssh = None;
        }
        terminal_config::apply(&mut tab, &app.settings);
        app.active = Some(tab.id.clone());
        app.tabs.push(tab);
        Ok(Self {
            app,
            ctx,
            path,
            _dir: dir,
        })
    }
    fn id(&self) -> String {
        self.app.tabs[0].id.clone()
    }
}
fn history(tab: &mut Tab, lines: usize) {
    if let Panel::Terminal { parser, .. } = &mut tab.panel {
        parser.set_size(3, 20);
        for n in 0..lines {
            parser.process(format!("line{n:02}\r\n").as_bytes());
        }
    }
}
#[test]
fn terminal_session_confirm_changes_live_history_without_reset_and_preserves_legacy_fields()
-> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    history(&mut f.app.tabs[0], 30);
    let (before, modes) = if let Panel::Terminal { parser, .. } = &mut f.app.tabs[0].panel {
        parser.process(b"\x1b[31mcolored\x1b[?2004h");
        let live = parser.screen().contents_formatted();
        parser.set_scrollback(10);
        (live, parser.screen().bracketed_paste())
    } else {
        unreachable!()
    };
    f.app.open_session_config(&f.id());
    let draft = f.app.session_config.as_mut().unwrap();
    assert!(!draft.rdp);
    draft.inherited = [false, false, true];
    draft.scrollback = "5".into();
    draft.file_mode = "ICON".into();
    let bridge = Bridge::new(&f.ctx);
    bridge.frame(&f.ctx, vec![], |ctx| f.app.session_config_dialog(ctx));
    assert!(bridge.text("打开方式:").is_err());
    bridge.click(&f.ctx, bridge.text("确定")?, |ctx| {
        f.app.session_config_dialog(ctx)
    });
    assert!(f.app.session_config.is_none());
    let saved = Store::load(&f.path)?;
    assert_eq!(saved.connections[0].number("scrollbackLines"), 5);
    assert_eq!(saved.connections[0].text("defaultFileViewMode"), "ICON");
    assert_eq!(saved.connections[0].text("rdpOpenMode"), "leave-untouched");
    assert_eq!(
        saved.connections[0].options.get("future"),
        f.app.store.connections[0].options.get("future")
    );
    assert!(!String::from_utf8_lossy(&std::fs::read(&f.path)?).contains("nested"));
    if let Panel::Terminal { parser, .. } = &mut f.app.tabs[0].panel {
        assert_eq!(parser.screen().history_len(), 5);
        assert_eq!(parser.screen().scrollback(), 5);
        assert_eq!(parser.screen().bracketed_paste(), modes);
        parser.set_scrollback(0);
        let live = parser.screen().contents_formatted();
        assert_eq!(live, before);
        assert!(parser.screen().contents().contains("colored"));
    }
    f.app.open_session_config(&f.id());
    f.app.session_config.as_mut().unwrap().inherited = [true, true, true];
    bridge.frame(&f.ctx, vec![], |ctx| f.app.session_config_dialog(ctx));
    bridge.click(&f.ctx, bridge.text("确定")?, |ctx| {
        f.app.session_config_dialog(ctx)
    });
    let saved = Store::load(&f.path)?;
    assert!(saved.connections[0].options["scrollbackLines"].is_null());
    assert!(saved.connections[0].options["defaultFileViewMode"].is_null());
    Ok(())
}
#[test]
fn terminal_session_cancel_close_and_external_conflict_never_write() -> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    let original = std::fs::read(&f.path)?;
    let bridge = Bridge::new(&f.ctx);
    for label in ["取消", "确定"] {
        f.app.open_session_config(&f.id());
        f.app.session_config.as_mut().unwrap().scrollback = "999".into();
        if label == "确定" {
            std::fs::write(&f.path, b"external edit")?;
        }
        bridge.frame(&f.ctx, vec![], |ctx| f.app.session_config_dialog(ctx));
        bridge.click(&f.ctx, bridge.text(label)?, |ctx| {
            f.app.session_config_dialog(ctx)
        });
        if label == "取消" {
            assert!(f.app.session_config.is_none());
            assert_eq!(std::fs::read(&f.path)?, original);
        } else {
            assert!(f.app.session_config.is_some());
            assert_eq!(std::fs::read(&f.path)?, b"external edit");
        }
    }
    f.app.close_tab(&f.id(), true);
    assert!(f.app.session_config.is_none());
    Ok(())
}
#[test]
fn terminal_global_updates_inherited_history_but_respects_overrides_and_copies_sessions()
-> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    let id = f.id();
    f.app
        .terminal_tab_action(&id, terminal_config::Action::Copy);
    assert_eq!(f.app.tabs.len(), 2);
    assert_ne!(f.app.tabs[0].id, f.app.tabs[1].id);
    assert_eq!(f.app.tabs[0].source_id, f.app.tabs[1].source_id);
    if let Panel::Terminal { connection, .. } = &mut f.app.tabs[1].panel {
        connection.set("scrollbackLines", 3);
    }
    for tab in &mut f.app.tabs {
        history(tab, 30);
    }
    let mut next = f.app.settings.clone();
    next.scrollback_lines = 7;
    next.ssh_terminal_font_size = 18.;
    f.app
        .save_settings(&settings::Form::from(&next), settings::Page::Ssh);
    assert!(f.app.error.is_none(), "{:?}", f.app.error);
    for (index, tab) in f.app.tabs.iter_mut().enumerate() {
        if let Panel::Terminal { parser, view, .. } = &mut tab.panel {
            assert_eq!(
                parser.screen().history_len(),
                if index == 0 { 7 } else { 3 }
            );
            assert_eq!(view.font_size, Some(18.));
            parser.process(b"\x1b[?1049hfullscreen\x1b[?1049l");
            assert_eq!(
                parser.screen().history_len(),
                if index == 0 { 7 } else { 3 }
            );
        }
    }
    f.app
        .terminal_tab_action(&id, terminal_config::Action::Global);
    assert_eq!(f.app.section, 4);
    assert!(f.app.settings_page == settings::Page::Ssh);
    assert_eq!(f.app.active.as_deref(), Some(f.app.tabs[1].id.as_str()));
    Ok(())
}
#[test]
fn ssh_tab_context_menu_matches_java_and_targets_the_clicked_inactive_tab() -> anyhow::Result<()> {
    let mut f = Fixture::new()?;
    // Owned inert transport under an SSH view; this test never opens a network session.
    if let Panel::Terminal { connection, .. } = &mut f.app.tabs[0].panel {
        connection.kind = Some("SSH".into());
    }
    let id = f.id();
    let ctx = f.ctx.clone();
    let draw = |ui: &mut Ui, app: &mut TomatoApp| {
        let response =
            super::super::tab_strip::header(ui, &app.tabs[0], &mut app.icons, false).response;
        let mut action = None;
        response.context_menu(|ui| {
            action = terminal_config::menu(ui, &app.tabs[0]);
        });
        if let Some(action) = action {
            app.terminal_tab_action(&id, action);
        }
    };
    let mut frame = |events| {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 600.),
                )),
                events,
                ..Default::default()
            },
            |ui| draw(ui, &mut f.app),
        );
        out.textures_delta.clear();
        out
    };
    frame(vec![]);
    let point = egui::pos2(50., 20.);
    for pressed in [true, false] {
        frame(vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    let out = frame(vec![]);
    let texts: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some(t.galley.job.text.as_str())
            } else {
                None
            }
        })
        .collect();
    for label in ["复制会话", "会话配置", "全局配置"] {
        assert!(texts.contains(&label), "{texts:?}");
    }
    let at = out
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == "会话配置").then_some(t.pos + t.galley.size() / 2.)
            } else {
                None
            }
        })
        .unwrap();
    for pressed in [true, false] {
        frame(vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]);
    }
    assert_eq!(f.app.session_config.as_ref().unwrap().tab, id);
    Ok(())
}
