//! Production pointer/keyboard controls, with isolated actors for committed changes.
use super::*;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use std::time::{Duration, Instant};
use tomato_services::terminal::{Terminal, ssh::tools::Entry};
mod s3;

struct Browser {
    ctx: egui::Context,
    view: Workspace,
    connection: tomato_core::Connection,
    icons: crate::icons::Icons,
    time: f64,
}
impl Browser {
    fn new(protocol: &str, mode: &str) -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut connection = tomato_core::Connection::new(protocol);
        connection.set("bucket", "fixture");
        let mut view = Workspace::standalone_files(mode, "/fixture");
        let pane = view.panes.get_mut(&Section::Files).unwrap();
        pane.refresh = false;
        pane.entries = ["first.txt", "second.txt"]
            .into_iter()
            .map(|name| Entry {
                name: name.into(),
                path: format!("/fixture/{name}"),
                directory: false,
                symlink: false,
                size: 7,
                modified: 0,
                permissions: 0,
            })
            .collect();
        Self {
            ctx,
            view,
            connection,
            icons: Default::default(),
            time: 10.,
        }
    }
    fn pane(&self) -> &crate::tabs::ssh::Pane {
        &self.view.panes[&Section::Files]
    }
    fn frame(&mut self, mut events: Vec<Event>) -> egui::FullOutput {
        self.time += 0.02;
        let modifiers = events
            .iter()
            .find_map(|e| match e {
                Event::PointerButton { modifiers, .. } | Event::Key { modifiers, .. } => {
                    Some(*modifiers)
                }
                _ => None,
            })
            .unwrap_or_default();
        events.insert(0, Event::ModifiersChanged(modifiers));
        let mut output = self.ctx.run_ui(
            egui::RawInput {
                time: Some(self.time),
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1200., 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                let rect = ui.max_rect();
                super::super::render(ui, rect, &mut self.view, &self.connection, &mut self.icons);
            },
        );
        output.textures_delta.clear();
        output
    }
    fn idle(&mut self, seconds: f64) -> egui::FullOutput {
        self.time += seconds;
        self.frame(vec![])
    }
    fn point(&mut self, label: &str) -> Pos2 {
        let out = self.frame(vec![]);
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.galley.job.text.trim_end_matches('/') == label.trim_end_matches('/') =>
                {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("actual {label} control missing in {}", self.view.file_view))
    }
    fn click(&mut self, point: Pos2, button: PointerButton, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button,
                    pressed,
                    modifiers,
                },
            ]);
        }
    }
    fn key(&mut self, key: Key) {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
    }
    fn select(&mut self, name: &str) -> Pos2 {
        self.idle(0.5);
        let point = self.point(name);
        self.click(point, PointerButton::Primary, Modifiers::NONE);
        point
    }
    fn start(&mut self, name: &str) {
        let point = self.select(name);
        assert!(self.pane().rename.is_none(), "a first click only selects");
        self.idle(0.5);
        self.click(point, PointerButton::Primary, Modifiers::NONE);
        assert!(
            self.pane()
                .rename
                .as_ref()
                .is_some_and(|edit| edit.due.is_some())
        );
        self.idle(0.31);
        assert!(
            self.pane()
                .rename
                .as_ref()
                .is_some_and(|edit| edit.due.is_none())
        );
        assert!(
            self.view.dialog.is_none(),
            "rename must not create a dialog"
        );
    }
    fn type_name(&mut self, value: &str) {
        self.frame(vec![Event::Text(value.into())]);
        assert_eq!(
            self.pane().rename.as_ref().unwrap().name,
            value,
            "the original name must be selected"
        );
    }
}

#[test]
fn all_file_browser_modes_select_then_click_rename_in_place_and_escape_releases_text_state() {
    for protocol in ["S3", "ALIYUN_OSS", "FTP", "SFTP", "SSH", "LOCAL_DIRECTORY"] {
        for mode in ["LIST", "ICON", "COLUMN"] {
            if protocol == "FTP" && mode == "COLUMN" {
                continue;
            }
            let mut b = Browser::new(protocol, mode);
            let second = b.point("second.txt");
            b.start("first.txt");
            assert_eq!(
                b.point("second.txt"),
                second,
                "editing must not change row/tile height"
            );
            let editor = b.ctx.memory(|m| m.focused()).expect("name field focus");
            b.type_name("中文 very long replacement name.txt");
            let draft = b.pane().rename.as_ref().unwrap();
            let rect = b
                .ctx
                .data(|data| {
                    data.get_temp::<egui::Rect>(egui::Id::new(("file-rename-field", &draft.from)))
                })
                .unwrap();
            assert!(
                rect.height() <= 26.1,
                "fixed row geometry: {protocol}/{mode}: {rect:?}"
            );
            assert!(rect.width() > 60., "name must not wrap into a tiny column");
            b.key(Key::Escape);
            assert!(b.pane().rename.is_none());
            assert!(b.view.dialog.is_none());
            assert!(
                egui::TextEdit::load_state(&b.ctx, editor).is_none(),
                "release undo strings after cancellation"
            );
            assert!(!b.view.busy(Section::Files));
        }
    }
}

#[test]
fn file_rename_context_menu_and_f2_use_the_same_inline_editor_and_buckets_are_excluded() {
    for mode in ["LIST", "ICON", "COLUMN"] {
        let mut b = Browser::new("S3", mode);
        let point = b.select("first.txt");
        b.click(point, PointerButton::Secondary, Modifiers::NONE);
        let rename = b.point("重命名");
        b.click(rename, PointerButton::Primary, Modifiers::NONE);
        b.frame(vec![]);
        assert!(
            b.pane()
                .rename
                .as_ref()
                .is_some_and(|edit| edit.due.is_none())
        );
        assert!(b.view.dialog.is_none());
        b.key(Key::Escape);
        b.select("first.txt");
        b.key(Key::F2);
        b.frame(vec![]);
        assert!(b.pane().rename.is_some());
        assert!(b.view.dialog.is_none());
        b.key(Key::Escape);
        b.connection.set("bucket", "");
        b.view.panes.get_mut(&Section::Files).unwrap().path = "/".into();
        b.select("first.txt");
        b.key(Key::F2);
        b.idle(0.4);
        assert!(b.pane().rename.is_none(), "buckets cannot be renamed");
    }
}

#[test]
fn file_rename_distinguishes_other_selection_double_click_modifiers_and_stale_context() {
    for mode in ["LIST", "ICON", "COLUMN"] {
        let mut b = Browser::new("SFTP", mode);
        b.select("first.txt");
        let second = b.point("second.txt");
        b.click(second, PointerButton::Primary, Modifiers::NONE);
        b.idle(0.4);
        assert!(
            b.pane().rename.is_none(),
            "selecting another item is not rename"
        );
        b.idle(0.5);
        b.click(second, PointerButton::Primary, Modifiers::CTRL);
        b.idle(0.4);
        assert!(b.pane().rename.is_none());
        let point = b.select("first.txt");
        // Keep the selection click outside egui's 600 ms triple-click window.
        b.idle(0.7);
        b.click(point, PointerButton::Primary, Modifiers::NONE);
        b.click(point, PointerButton::Primary, Modifiers::NONE);
        assert!(b.view.busy(Section::Files), "double click opens: {mode}");
        b.click(point, PointerButton::Primary, Modifiers::NONE);
        b.idle(0.4);
        assert!(
            b.pane().rename.is_none(),
            "double click must open instead: {mode}"
        );
        assert!(
            b.view.busy(Section::Files),
            "triple click keeps the open request: {mode}"
        );

        let mut b = Browser::new("SFTP", mode);
        b.start("first.txt");
        let editor = b.ctx.memory(|m| m.focused()).unwrap();
        b.view.panes.get_mut(&Section::Files).unwrap().filter = "second".into();
        b.frame(vec![]);
        assert!(b.pane().rename.is_none());
        assert!(egui::TextEdit::load_state(&b.ctx, editor).is_none());
        assert!(!b.view.busy(Section::Files));
    }
}

async fn wait(
    b: &mut Browser,
    terminal: &mut Terminal,
    done: impl Fn(&Browser) -> bool,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        b.view.poll(terminal, &b.ctx, true);
        if done(b) {
            return Ok(());
        }
        anyhow::ensure!(
            Instant::now() < deadline,
            "owned file actor timed out: {}",
            b.pane().status
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test]
async fn inline_file_rename_enter_and_focus_loss_commit_through_the_real_local_actor()
-> anyhow::Result<()> {
    for mode in ["LIST", "ICON", "COLUMN"] {
        for finish in ["enter", "outside", "batched-outside"] {
            let directory = tempfile::tempdir()?;
            std::fs::write(directory.path().join("first.txt"), b"actual original bytes")?;
            std::fs::write(directory.path().join("second.txt"), b"unrelated file")?;
            let mut b = Browser::new("LOCAL_DIRECTORY", mode);
            b.connection.set(
                "localDirectoryPath",
                directory.path().to_string_lossy().as_ref(),
            );
            let mut terminal =
                Terminal::from_file_session(tomato_services::files::session::FileSession::start(
                    &tokio::runtime::Handle::current(),
                    b.connection.clone(),
                ));
            let pane = b.view.panes.get_mut(&Section::Files).unwrap();
            pane.entries.clear();
            pane.path = directory.path().to_string_lossy().replace('\\', "/");
            pane.refresh = true;
            wait(&mut b, &mut terminal, |b| b.pane().entries.len() == 2).await?;
            b.start("first.txt");
            b.type_name("renamed.txt");
            match finish {
                "enter" => b.key(Key::Enter),
                "outside" => b.click(
                    egui::pos2(1100., 700.),
                    PointerButton::Primary,
                    Modifiers::NONE,
                ),
                _ => {
                    let point = b.point("second.txt");
                    b.frame(vec![
                        Event::PointerMoved(point),
                        Event::PointerButton {
                            pos: point,
                            button: PointerButton::Primary,
                            pressed: true,
                            modifiers: Modifiers::NONE,
                        },
                        Event::PointerButton {
                            pos: point,
                            button: PointerButton::Primary,
                            pressed: false,
                            modifiers: Modifiers::NONE,
                        },
                    ]);
                }
            }
            assert!(b.pane().rename.is_none());
            wait(&mut b, &mut terminal, |b| {
                b.pane().entries.iter().any(|e| e.name == "renamed.txt")
                    && !b.view.busy(Section::Files)
            })
            .await?;
            assert_eq!(
                std::fs::read(directory.path().join("renamed.txt"))?,
                b"actual original bytes"
            );
            assert!(!directory.path().join("first.txt").exists());
            assert_eq!(
                std::fs::read(directory.path().join("second.txt"))?,
                b"unrelated file"
            );
            assert!(b.view.dialog.is_none());
        }
    }
    Ok(())
}

#[tokio::test]
async fn inline_column_rename_of_an_open_ancestor_refreshes_the_existing_parent()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    std::fs::create_dir(directory.path().join("folder"))?;
    std::fs::write(
        directory.path().join("folder/inside.txt"),
        b"nested content",
    )?;
    let mut b = Browser::new("LOCAL_DIRECTORY", "COLUMN");
    b.connection.set(
        "localDirectoryPath",
        directory.path().to_string_lossy().as_ref(),
    );
    let mut terminal =
        Terminal::from_file_session(tomato_services::files::session::FileSession::start(
            &tokio::runtime::Handle::current(),
            b.connection.clone(),
        ));
    let pane = b.view.panes.get_mut(&Section::Files).unwrap();
    pane.entries.clear();
    pane.path = directory.path().to_string_lossy().replace('\\', "/");
    pane.refresh = true;
    wait(&mut b, &mut terminal, |b| {
        b.pane().entries.iter().any(|e| e.name == "folder")
    })
    .await?;
    b.select("folder");
    wait(&mut b, &mut terminal, |b| {
        b.pane().entries.iter().any(|e| e.name == "inside.txt")
    })
    .await?;
    b.idle(0.5);
    let point = b.point("folder");
    b.click(point, PointerButton::Primary, Modifiers::NONE);
    b.idle(0.31);
    assert!(b.pane().rename.is_some());
    b.type_name("renamed-folder");
    b.key(Key::Enter);
    wait(&mut b, &mut terminal, |b| {
        b.pane().entries.iter().any(|e| e.name == "renamed-folder") && !b.view.busy(Section::Files)
    })
    .await?;
    assert_eq!(
        std::fs::read(directory.path().join("renamed-folder/inside.txt"))?,
        b"nested content"
    );
    assert!(b.pane().columns.is_empty());
    assert!(!directory.path().join("folder").exists());
    Ok(())
}
