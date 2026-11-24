use super::*;
use crate::tabs::redis::State;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};

struct Harness {
    ctx: egui::Context,
    tab: Tab,
    time: f64,
    output: Option<egui::FullOutput>,
}
impl Harness {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut tab = crate::tabs::create_tab(tomato_core::Connection::new("REDIS"), None).unwrap();
        let Panel::Redis { view, .. } = &mut tab.panel else {
            unreachable!()
        };
        view.accept(
            tomato_services::redis_data::Output::Keys {
                keys: vec!["a".into(), "b".into()],
                cursor: 0,
            },
            0.,
        );
        view.accept(
            tomato_services::redis_data::Output::Value {
                key: "a".into(),
                kind: "string".into(),
                text: "old value".into(),
                ttl: -1,
            },
            0.,
        );
        let mut h = Self {
            ctx,
            tab,
            time: 0.,
            output: None,
        };
        h.frame(vec![]);
        h.frame(vec![]);
        h
    }
    fn view(&mut self) -> &mut State {
        let Panel::Redis { view, .. } = &mut self.tab.panel else {
            unreachable!()
        };
        view
    }
    fn frame(&mut self, events: Vec<Event>) -> Option<Action> {
        self.time += 0.1;
        let mut action = None;
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(900., 600.),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let Some(Intent::Command(command)) =
                        render(ui, &mut self.tab, &mut crate::icons::Icons::default())
                        && let Command::RedisData { action: next, .. } = *command
                    {
                        action = Some(next);
                    }
                });
            },
        );
        output.textures_delta.clear();
        self.output = Some(output);
        action
    }
    fn text(&self, caption: &str) -> Pos2 {
        self.output
            .as_ref()
            .unwrap()
            .shapes
            .iter()
            .rev()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text() == caption => {
                    Some(t.pos + t.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing actual control {caption}"))
    }
    fn click(&mut self, pos: Pos2, button: PointerButton) -> Option<Action> {
        self.frame(vec![
            Event::PointerMoved(pos),
            Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ]);
        self.frame(vec![Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers: Modifiers::NONE,
        }])
    }
    fn replace(&mut self, text: &str) {
        self.frame(vec![Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers {
                ctrl: true,
                command: true,
                ..Modifiers::NONE
            },
        }]);
        self.frame(vec![Event::Text(text.into())]);
    }
}

#[test]
fn right_click_rename_uses_selected_key_without_reading_or_mutating_the_previous_value() {
    let mut h = Harness::new();
    let b = h.text("b");
    assert!(h.click(b, PointerButton::Secondary).is_none());
    h.frame(vec![]);
    h.frame(vec![]);
    assert_eq!(h.view().current.as_deref(), Some("a"));
    assert_eq!(h.view().single_selected(), Some("b"));
    let rename = h.text("重命名");
    h.click(rename, PointerButton::Primary);
    h.frame(vec![]);
    h.frame(vec![]);
    assert!(matches!(&h.view().dialog,Some(Dialog::Rename {key,..})if key=="b"));
    h.replace("renamed");
    let editor_id = h
        .ctx
        .memory(|m| m.focused())
        .expect("Actual rename editor must own focus");
    assert!(egui::widgets::text_edit::TextEditState::load(&h.ctx, editor_id).is_some());
    let confirm = h.text("确定");
    assert!(
        matches!(h.click(confirm,PointerButton::Primary),Some(Action::Rename {key,name})if key=="b"&&name=="renamed")
    );
    assert_eq!(h.view().text, "old value");
    h.view().accept(
        tomato_services::redis_data::Output::Changed {
            key: Some("renamed".into()),
            message: "saved".into(),
        },
        0.,
    );
    h.frame(vec![]);
    assert!(
        egui::widgets::text_edit::TextEditState::load(&h.ctx, editor_id).is_none(),
        "Completed modal must release its editor and undo snapshots"
    );
}

#[test]
fn modal_delete_freezes_selection_and_invalid_ttl_preserves_the_edit_for_retry() {
    let mut h = Harness::new();
    let b = h.text("b");
    h.view().selected = std::collections::BTreeSet::from(["a".into(), "b".into()]);
    h.view().open_delete();
    h.frame(vec![]);
    h.frame(vec![]);
    h.click(b, PointerButton::Primary);
    assert_eq!(
        h.view().selected.len(),
        2,
        "Background rows must be disabled"
    );
    let confirm = h.text("删除");
    assert!(
        matches!(h.click(confirm,PointerButton::Primary),Some(Action::Delete {keys})if keys==["a","b"])
    );
    h.view().dialog = None;
    h.view().selected = std::collections::BTreeSet::from(["b".into()]);
    h.view().open_ttl();
    h.frame(vec![]);
    h.frame(vec![]);
    h.replace("-2");
    let confirm = h.text("确定");
    assert!(h.click(confirm, PointerButton::Primary).is_none());
    assert!(h.view().failure.is_some());
    assert!(matches!(&h.view().dialog,Some(Dialog::Ttl {key,seconds})if key=="b"&&seconds=="-2"));
    h.frame(vec![]);
    h.frame(vec![]);
    let dismiss = h.text("确定");
    h.click(dismiss, PointerButton::Primary);
    assert!(h.view().failure.is_none());
    h.frame(vec![]);
    h.frame(vec![]);
    h.replace(" 600 ");
    let confirm = h.text("确定");
    assert!(
        matches!(h.click(confirm,PointerButton::Primary),Some(Action::Ttl {key,seconds:600})if key=="b")
    );
}

#[test]
fn keyboard_tree_selection_scrolls_offscreen_and_copy_is_scoped_to_tree_focus() {
    let mut h = Harness::new();
    h.view().followup = Some(Action::Read { key: "a".into() });
    assert!(matches!(h.frame(vec![]), Some(Action::Read { key }) if key == "a"));
    h.text("Key: a"); // Queuing a background continuation must still paint this frame.
    h.view().accept(
        tomato_services::redis_data::Output::Keys {
            keys: (0..5000).map(|i| format!("key{i:04}")).collect(),
            cursor: 0,
        },
        0.,
    );
    h.frame(vec![]);
    let first = h.text("a");
    h.click(first, PointerButton::Primary);
    let control = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };
    h.frame(vec![
        Event::ModifiersChanged(control),
        Event::Key {
            key: Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: control,
        },
    ]);
    assert_eq!(h.view().focused.as_deref(), Some("b"));
    assert_eq!(
        h.view().single_selected(),
        Some("a"),
        "Actual Java Ctrl+Down changes focus, not selection"
    );
    h.frame(vec![
        Event::ModifiersChanged(control),
        Event::Key {
            key: Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: control,
        },
    ]);
    assert_eq!(
        h.view().selected,
        std::collections::BTreeSet::from(["a".into(), "b".into()])
    );
    h.frame(vec![
        Event::ModifiersChanged(Modifiers::NONE),
        Event::Key {
            key: Key::End,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.frame(vec![]);
    assert_eq!(h.view().single_selected(), Some("key4999"));
    h.text("key4999");
    h.frame(vec![Event::Copy]);
    assert!(
        h.output
            .as_ref()
            .unwrap()
            .platform_output
            .commands
            .iter()
            .any(|c| matches!(c,egui::OutputCommand::CopyText(t)if t=="key4999"))
    );
    h.ctx
        .memory_mut(|m| m.request_focus(egui::Id::new("another-editor")));
    h.frame(vec![Event::Key {
        key: Key::A,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        },
    }]);
    assert_eq!(h.view().selected.len(), 1);
    assert!(
        h.output.as_ref().unwrap().shapes.len() < 500,
        "Only visible rows may paint"
    );
}

#[test]
#[ignore = "Requires TOMATO_TEST_REDIS_SERVER; starts only its own isolated loopback child"]
fn real_redis_widget_mutations_readback_and_complete_multi_batch_scan() -> anyhow::Result<()> {
    use tomato_services::redis_data::{self, Output};
    let executable = std::path::PathBuf::from(
        std::env::var_os("TOMATO_TEST_REDIS_SERVER").expect("Owned Redis executable required"),
    );
    assert!(executable.is_absolute() && executable.is_file());
    let directory = tempfile::tempdir()?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let mut command = std::process::Command::new(executable);
    command
        .current_dir(directory.path())
        .args([
            "--bind",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "--save",
            "",
            "--appendonly",
            "no",
            "--maxmemory",
            "32mb",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let _owned = Child(command.spawn()?);
    let runtime = tokio::runtime::Runtime::new()?;
    let mut connection = tomato_core::Connection::new("REDIS");
    connection.set("host", "127.0.0.1");
    connection.set("port", port);
    connection.set("redisDatabase", 3);
    for _ in 0..100 {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let run = |action| runtime.block_on(redis_data::execute(&connection, action));
    for (key, text) in [("a", "alpha"), ("b", "beta"), ("c", "untouched")] {
        run(Action::Write {
            key: key.into(),
            kind: "string".into(),
            text: text.into(),
            create: true,
        })?;
    }
    let mut h = Harness::new();
    let Panel::Redis {
        connection: target,
        view,
    } = &mut h.tab.panel
    else {
        unreachable!()
    };
    *target = std::sync::Arc::new(connection.clone());
    view.database = 3;
    h.view().accept(run(Action::Read { key: "a".into() })?, 0.);
    h.frame(vec![]);
    let b = h.text("b");
    h.click(b, PointerButton::Secondary);
    h.frame(vec![]);
    h.frame(vec![]);
    let rename = h.text("重命名");
    h.click(rename, PointerButton::Primary);
    h.frame(vec![]);
    h.frame(vec![]);
    h.replace("renamed");
    let accept = h.text("确定");
    let action = h
        .click(accept, PointerButton::Primary)
        .expect("Actual rename button must issue action");
    assert!(matches!(&action,Action::Rename {key,name}if key=="b"&&name=="renamed"));
    h.view().accept(run(action)?, 0.);
    let drain = |h: &mut Harness| -> anyhow::Result<usize> {
        let mut scans = 0;
        for _ in 0..4096 {
            let Some(action) = h.frame(vec![]) else {
                return Ok(scans);
            };
            if matches!(action, Action::Scan { .. }) {
                scans += 1;
            }
            h.view().accept(run(action)?, 0.);
        }
        anyhow::bail!("Scan did not finish")
    };
    drain(&mut h)?;
    assert!(matches!(run(Action::Read {key:"a".into()})?,Output::Value {text,..}if text=="alpha"));
    assert!(matches!(run(Action::Read {key:"b".into()})?,Output::Value {kind,..}if kind=="none"));
    assert!(
        matches!(run(Action::Read {key:"renamed".into()})?,Output::Value {text,..}if text=="beta")
    );
    h.view().accept(run(Action::Read { key: "a".into() })?, 0.);
    h.view().selected = std::collections::BTreeSet::from(["renamed".into()]);
    h.frame(vec![]);
    let ttl = h.text("TTL");
    h.click(ttl, PointerButton::Primary);
    h.frame(vec![]);
    h.frame(vec![]);
    h.replace("600");
    let accept = h.text("确定");
    let action = h
        .click(accept, PointerButton::Primary)
        .expect("Actual TTL button must issue action");
    assert!(matches!(&action,Action::Ttl {key,seconds:600}if key=="renamed"));
    h.view().accept(run(action)?, 0.);
    drain(&mut h)?;
    assert!(matches!(
        run(Action::Read { key: "a".into() })?,
        Output::Value { ttl: -1, .. }
    ));
    assert!(
        matches!(run(Action::Read {key:"renamed".into()})?,Output::Value {ttl,..}if (590..=600).contains(&ttl))
    );
    h.view().selected = std::collections::BTreeSet::from(["a".into(), "renamed".into()]);
    h.view().open_delete();
    h.frame(vec![]);
    h.frame(vec![]);
    let accept = h.text("删除");
    let action = h
        .click(accept, PointerButton::Primary)
        .expect("Actual delete button must issue action");
    h.view().accept(run(action)?, 0.);
    drain(&mut h)?;
    for key in ["a", "renamed"] {
        assert!(
            matches!(run(Action::Read {key:key.into()})?,Output::Value {kind,..}if kind=="none")
        );
    }
    assert!(
        matches!(run(Action::Read {key:"c".into()})?,Output::Value {text,..}if text=="untouched")
    );
    runtime.block_on(redis_data::raw_command(
        &connection,
        &[
            "EVAL".into(),
            "for i=1,1100 do redis.call('SET','search:'..i,'value') end return 1100".into(),
            "0".into(),
        ],
    ))?;
    h.view().pattern = "search:*".into();
    let action = h.view().begin_scan();
    h.view().accept(run(action)?, 0.);
    let scans = drain(&mut h)?;
    assert!(
        scans > 0,
        "A complete tree must load subsequent actual SCAN batches"
    );
    assert_eq!(h.view().keys.len(), 1100);
    assert_eq!(h.view().cursor, 0);
    assert!(h.view().keys.iter().all(|key| key.starts_with("search:")));
    println!(
        "Owned actual Redis: widget rename, TTL, batch delete, unchanged neighbour and 1100-key complete server scan passed"
    );
    Ok(())
}
