use super::*;
use egui::{Event, Modifiers, PointerButton, Pos2};
use tomato_services::terminal::ssh::tools::Entry;

struct Browser {
    ctx: egui::Context,
    view: Workspace,
    connection: tomato_core::Connection,
    icons: crate::icons::Icons,
}
impl Browser {
    fn new(protocol: &str, mode: &str) -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut view = Workspace::standalone_files(mode, "/fixture");
        let pane = view.panes.get_mut(&Section::Files).unwrap();
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
            connection: tomato_core::Connection::new(protocol),
            icons: Default::default(),
        }
    }
    fn frame(&mut self, mut events: Vec<Event>, modifiers: Modifiers) -> egui::FullOutput {
        events.insert(0, Event::ModifiersChanged(modifiers));
        let mut output = self.ctx.run_ui(
            egui::RawInput {
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
        // This headless control test inspects shapes rather than uploading textures.
        output.textures_delta.clear();
        output
    }
    fn pointer(&mut self, point: Pos2, pressed: Option<bool>, modifiers: Modifiers) {
        let mut events = vec![Event::PointerMoved(point)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed,
                modifiers,
            });
        }
        self.frame(events, modifiers);
    }
    fn pane(&self) -> &crate::tabs::ssh::Pane {
        &self.view.panes[&Section::Files]
    }
}

#[test]
fn file_views_fill_the_page_and_drag_select_shrink_release_and_add() {
    for protocol in ["S3", "SFTP", "ALIYUN_OSS", "FTP", "LOCAL_DIRECTORY"] {
        for mode in ["ICON", "LIST", "COLUMN"] {
            if protocol == "FTP" && mode == "COLUMN" {
                continue;
            }
            let mut b = Browser::new(protocol, mode);
            let output = b.frame(vec![], Modifiers::NONE);
            let first = output
                .shapes
                .iter()
                .find_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        (t.galley.job.text.starts_with("first.txt")).then_some(t.pos)
                    } else {
                        None
                    }
                })
                .expect("real file label");
            // A short directory must still own the blank region near the footer.
            let start = egui::pos2(1100., 700.);
            b.pointer(start, Some(true), Modifiers::NONE);
            assert!(
                b.pane().file_drag.is_some(),
                "{protocol}/{mode}: bottom blank area missing"
            );
            let end = egui::pos2(0., first.y - 1.);
            b.pointer(end, None, Modifiers::NONE);
            assert_eq!(b.pane().selected, [0, 1].into(), "{protocol}/{mode}");
            b.pointer(start - egui::vec2(1., 1.), None, Modifiers::NONE);
            assert!(
                b.pane().selected.is_empty(),
                "shrinking the box must remove stale hits"
            );
            b.pointer(end, Some(false), Modifiers::NONE);
            assert!(b.pane().file_drag.is_none());
            assert_eq!(b.pane().selected, [0, 1].into());

            b.view.panes.get_mut(&Section::Files).unwrap().selected = [1].into();
            b.pointer(start, Some(true), Modifiers::CTRL);
            b.pointer(end, None, Modifiers::CTRL);
            b.pointer(end, Some(false), Modifiers::CTRL);
            assert_eq!(b.pane().selected, [0, 1].into());
            // Ordinary blank click clears selection, without opening anything.
            b.pointer(start, Some(true), Modifiers::NONE);
            b.pointer(start, Some(false), Modifiers::NONE);
            assert!(b.pane().selected.is_empty());
            b.frame(
                vec![Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::CTRL,
                }],
                Modifiers::CTRL,
            );
            assert_eq!(
                b.pane().selected,
                [0, 1].into(),
                "keyboard selection after a blank click: {protocol}/{mode}"
            );
        }
    }
}

#[test]
fn file_range_cancels_when_filter_changes_and_does_not_start_on_an_item() {
    let mut b = Browser::new("S3", "LIST");
    let output = b.frame(vec![], Modifiers::NONE);
    let point = output
        .shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == "first.txt").then_some(t.pos + egui::vec2(5., 5.))
            } else {
                None
            }
        })
        .unwrap();
    b.pointer(point, Some(true), Modifiers::NONE);
    assert!(b.pane().file_drag.is_none());
    b.pointer(point, Some(false), Modifiers::NONE);
    assert_eq!(b.pane().selected, [0].into());
    let blank = egui::pos2(1000., 700.);
    b.pointer(blank, Some(true), Modifiers::NONE);
    assert!(b.pane().file_drag.is_some());
    b.view.panes.get_mut(&Section::Files).unwrap().filter = "second".into();
    b.pointer(point, None, Modifiers::NONE);
    assert!(b.pane().file_drag.is_none());
    assert!(b.pane().selected.is_empty());
    b.pointer(point, Some(false), Modifiers::NONE);
}

#[test]
fn file_views_empty_space_and_large_directories_do_not_create_blank_rows() {
    for mode in ["ICON", "LIST", "COLUMN"] {
        let mut b = Browser::new("S3", mode);
        b.view
            .panes
            .get_mut(&Section::Files)
            .unwrap()
            .entries
            .clear();
        b.frame(vec![], Modifiers::NONE);
        let blank = egui::pos2(if mode == "COLUMN" { 190. } else { 1100. }, 700.);
        b.pointer(blank, Some(true), Modifiers::NONE);
        assert!(
            b.pane().file_drag.is_some(),
            "empty {mode} must fill the content height"
        );
        b.pointer(blank, Some(false), Modifiers::NONE);
        let pane = b.view.panes.get_mut(&Section::Files).unwrap();
        pane.entries = (0..10_000)
            .map(|index| Entry {
                name: format!("file-{index}.txt"),
                path: format!("/fixture/{index}"),
                directory: false,
                symlink: false,
                size: 0,
                modified: 0,
                permissions: 0,
            })
            .collect();
        let output = b.frame(vec![], Modifiers::NONE);
        assert!(
            output.shapes.len() < 800,
            "{mode} must paint only visible items"
        );
    }
}

#[test]
fn file_icon_range_keeps_its_content_anchor_while_edge_scrolling() {
    let mut b = Browser::new("S3", "ICON");
    b.view.panes.get_mut(&Section::Files).unwrap().entries = (0..1000)
        .map(|index| Entry {
            name: format!("file-{index}.txt"),
            path: format!("/fixture/{index}"),
            directory: false,
            symlink: false,
            size: 0,
            modified: 0,
            permissions: 0,
        })
        .collect();
    b.frame(vec![], Modifiers::NONE);
    b.frame(
        vec![
            Event::PointerMoved(egui::pos2(600., 400.)),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0., -20_000.),
                modifiers: Modifiers::NONE,
            },
        ],
        Modifiers::NONE,
    );
    let first_visible = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    t.galley
                        .job
                        .text
                        .strip_prefix("file-")?
                        .strip_suffix(".txt")?
                        .parse::<usize>()
                        .ok()
                } else {
                    None
                }
            })
            .min()
            .expect("visible file label")
    };
    let mut before = 0;
    for _ in 0..60 {
        before = first_visible(&b.frame(vec![], Modifiers::NONE));
    }
    assert!(before > 100, "wheel must reach the lower directory rows");
    let last_y = |output: &egui::FullOutput| {
        output
            .shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    (t.galley.job.text == "file-999.txt").then_some(t.pos.y)
                } else {
                    None
                }
            })
            .expect("last file must remain visible at the bottom")
    };
    let bottom_y = last_y(&b.frame(vec![], Modifiers::NONE));
    for _ in 0..3 {
        b.frame(
            vec![Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: egui::vec2(0., -20_000.),
                modifiers: Modifiers::NONE,
            }],
            Modifiers::NONE,
        );
        for _ in 0..30 {
            b.frame(vec![], Modifiers::NONE);
        }
    }
    let repeated_y = last_y(&b.frame(vec![], Modifiers::NONE));
    assert!(
        (bottom_y - repeated_y).abs() <= 1.,
        "bottom padding must not grow on repeated scrolling: {bottom_y} -> {repeated_y}"
    );
    let blank = egui::pos2(1100., 700.);
    b.pointer(blank, Some(true), Modifiers::NONE);
    let anchor = b
        .pane()
        .file_drag
        .as_ref()
        .expect("blank final icon row")
        .start;
    let edge = egui::pos2(100., 10.);
    b.pointer(edge, None, Modifiers::NONE);
    let mut after = before;
    for _ in 0..48 {
        after = first_visible(&b.frame(vec![], Modifiers::NONE));
    }
    assert!(
        after < before,
        "holding the pointer at the upper edge must scroll: {before} -> {after}"
    );
    assert_eq!(b.pane().file_drag.as_ref().unwrap().start, anchor);
    assert!(!b.pane().selected.is_empty());
    b.pointer(edge, Some(false), Modifiers::NONE);
    assert!(b.pane().file_drag.is_none());
}
