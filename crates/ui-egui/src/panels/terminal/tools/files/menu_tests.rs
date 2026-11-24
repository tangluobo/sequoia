//! Real widgets and input routing, including nonempty background context menus.
use super::*;
use egui::{Event, Key, Modifiers, PointerButton, Pos2};

#[test]
fn dragging_file_names_dispatches_export_from_all_views_instead_of_range_selection() {
    for mode in ["LIST", "ICON", "COLUMN"] {
        for select_first in [false, true] {
            let mut browser = Browser::new("S3", mode);
            let origin = browser.point("readme.md");
            if select_first {
                browser.click(origin, PointerButton::Primary, Modifiers::NONE);
            }
            browser.frame(vec![
                Event::PointerMoved(origin),
                Event::PointerButton {
                    pos: origin,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ]);
            browser.frame(vec![Event::PointerMoved(origin + egui::vec2(45., 0.))]);
            let pane = &browser.view.panes[&Section::Files];
            assert!(
                pane.export
                    .as_ref()
                    .is_some_and(|export| export.path == "/fixture/readme.md"),
                "{mode}, selected first={select_first}: actual file drag must prepare an export"
            );
            assert!(
                pane.file_drag.is_none(),
                "item drag must not become a range selection"
            );
            assert!(
                browser.view.busy(Section::Files),
                "export must reach the actor queue"
            );
        }
    }
}
#[test]
fn dragging_a_selected_file_preserves_the_complete_multi_selection_in_every_view() {
    for mode in ["LIST", "ICON", "COLUMN"] {
        for dragged in ["readme.md", "third.txt"] {
            let mut browser = Browser::new("S3", mode);
            let origin = browser.point("readme.md");
            browser.click(origin, PointerButton::Primary, Modifiers::NONE);
            let last = browser.point("third.txt");
            browser.click(last, PointerButton::Primary, Modifiers::CTRL);
            assert_eq!(browser.view.panes[&Section::Files].selected.len(), 2);
            let origin = browser.point(dragged);
            browser.frame(vec![
                Event::PointerMoved(origin),
                Event::PointerButton {
                    pos: origin,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
            ]);
            browser.frame(vec![Event::PointerMoved(origin + egui::vec2(45., 0.))]);
            let pane = &browser.view.panes[&Section::Files];
            let export = pane
                .export
                .as_ref()
                .expect("multi-file export reaches real actor");
            assert_eq!(
                export
                    .request
                    .items()
                    .iter()
                    .map(|i| i.path.as_str())
                    .collect::<Vec<_>>(),
                ["/fixture/readme.md", "/fixture/third.txt"],
                "{mode}"
            );
            assert_eq!(pane.selected, [0, 2].into());
            assert_eq!(export.path, export.request.path);
        }
    }
}

struct Browser {
    ctx: egui::Context,
    view: Workspace,
    connection: tomato_core::Connection,
    icons: crate::icons::Icons,
    time: f64,
    intent: Option<Intent>,
}
impl Browser {
    fn new(kind: &str, mode: &str) -> Self {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut view = Workspace::standalone_files(mode, "/fixture");
        let pane = view.panes.get_mut(&Section::Files).unwrap();
        pane.refresh = false;
        pane.entries = ["readme.md", "picture.png", "third.txt"]
            .into_iter()
            .map(|name| tomato_services::files::operations::Entry {
                name: name.into(),
                path: format!("/fixture/{name}"),
                directory: false,
                symlink: false,
                size: 5,
                modified: 0,
                permissions: 0,
            })
            .collect();
        let mut connection = tomato_core::Connection::new(kind);
        connection.set("bucket", "fixture");
        Self {
            ctx,
            view,
            connection,
            icons: Default::default(),
            time: 10.,
            intent: None,
        }
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
        let mut out = self.ctx.run_ui(
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
                self.intent = super::super::render(
                    ui,
                    ui.max_rect(),
                    &mut self.view,
                    &self.connection,
                    &mut self.icons,
                );
            },
        );
        out.textures_delta.clear();
        out
    }
    fn point(&mut self, name: &str) -> Pos2 {
        self.frame(vec![])
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == name => {
                    Some(t.pos + t.galley.size() / 2.)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing widget: {name}"))
    }
    fn click(&mut self, p: Pos2, button: PointerButton, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(p),
                Event::PointerButton {
                    pos: p,
                    button,
                    pressed,
                    modifiers,
                },
            ]);
        }
    }
    fn labels(&mut self) -> Vec<String> {
        self.frame(vec![])
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.job.text.clone()),
                _ => None,
            })
            .collect()
    }
    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }]);
    }
}
#[test]
fn all_file_views_share_java_markdown_menu_and_nonempty_background_actions() {
    for (kind, mode) in [
        ("SFTP", "ICON"),
        ("SFTP", "LIST"),
        ("SFTP", "COLUMN"),
        ("S3", "ICON"),
        ("ALIYUN_OSS", "LIST"),
        ("FTP", "LIST"),
    ] {
        let mut b = Browser::new(kind, mode);
        let p = b.point("readme.md");
        b.click(p, PointerButton::Secondary, Modifiers::NONE);
        let labels = b.labels();
        assert!(
            labels.iter().any(|s| s == "下载..."),
            "{kind}/{mode}: {labels:?}"
        );
        assert_eq!(labels.iter().any(|s| s == "编辑 Markdown"), kind != "FTP");
        assert_eq!(labels.iter().any(|s| s == "创建文件"), kind != "FTP");
        assert!(labels.iter().any(|s| s == "视图"));
        b.key(Key::Escape, Modifiers::NONE);
        b.frame(vec![]);
        b.click(
            egui::pos2(if mode == "COLUMN" { 100. } else { 1100. }, 650.),
            PointerButton::Secondary,
            Modifiers::NONE,
        );
        let labels = b.labels();
        assert!(
            labels.iter().any(|s| s == "新建目录"),
            "nonempty blank {mode}: {labels:?}"
        );
        assert!(!labels.iter().any(|s| s == "下载..."));
        assert!(b.view.panes[&Section::Files].selected.is_empty());
    }
}

#[test]
fn detail_header_resize_keeps_total_width_rows_and_selection_and_sort_still_works() {
    let mut b = Browser::new("SFTP", "LIST");
    let row = b.point("readme.md");
    b.click(row, PointerButton::Primary, Modifiers::NONE);
    let old = b.point("大小");
    let boundary = egui::pos2(old.x - 50., old.y);
    b.frame(vec![
        Event::PointerMoved(boundary),
        Event::PointerButton {
            pos: boundary,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    b.frame(vec![Event::PointerMoved(boundary - egui::vec2(70., 0.))]);
    b.frame(vec![Event::PointerButton {
        pos: boundary - egui::vec2(70., 0.),
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    let pane = &b.view.panes[&Section::Files];
    let widths = pane
        .file_widths
        .as_ref()
        .expect("actual header drag must resize columns");
    assert!(widths[1] > 150.);
    assert!((widths.iter().sum::<f32>() - pane.content_bounds.unwrap().width()).abs() < 0.1);
    assert_eq!(pane.selected, [0].into());
    assert!(pane.file_sort.is_none());
    assert_eq!(
        b.point("readme.md").y,
        row.y,
        "resizing must keep the row height"
    );
    let size = b.point("大小");
    assert!(size.x < old.x);
    b.click(size, PointerButton::Primary, Modifiers::NONE);
    assert_eq!(b.view.panes[&Section::Files].file_sort, Some((1, false)));
    assert!(b.intent.is_none());
}

#[test]
fn detail_header_stays_visible_after_scrolling_without_painting_hidden_rows() {
    let mut b = Browser::new("SFTP", "LIST");
    let template = b.view.panes[&Section::Files].entries[0].clone();
    b.view.panes.get_mut(&Section::Files).unwrap().entries = (0..300)
        .map(|i| {
            let mut entry = template.clone();
            entry.name = format!("row-{i:03}.txt");
            entry.path = format!("/fixture/{}", entry.name);
            entry
        })
        .collect();
    let header = b.point("名称");
    b.frame(vec![
        Event::PointerMoved(egui::pos2(700., 400.)),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0., -950.),
            modifiers: Modifiers::NONE,
        },
    ]);
    for _ in 0..35 {
        b.frame(vec![]);
    }
    assert_eq!(b.point("名称").y, header.y);
    let out = b.frame(vec![]);
    let rows = out
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.job.text.starts_with("row-") => Some(t),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        !rows.is_empty() && rows.len() < 40,
        "only visible file rows may be painted"
    );
    assert!(!rows.iter().any(|t| t.galley.job.text == "row-000.txt"));
    assert!(rows.iter().all(|t| t.pos.y > header.y));
}
#[test]
fn file_keyboard_selects_visible_order_and_menu_opens_real_markdown_intent() {
    for mode in ["ICON", "LIST", "COLUMN"] {
        let mut b = Browser::new("SFTP", mode);
        let p = b.point("readme.md");
        b.click(p, PointerButton::Primary, Modifiers::NONE);
        b.key(Key::End, Modifiers::SHIFT);
        assert_eq!(b.view.panes[&Section::Files].selected.len(), 3);
        b.key(Key::Home, Modifiers::NONE);
        assert_eq!(
            b.view.panes[&Section::Files]
                .selected
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            [0]
        );
        b.key(Key::Enter, Modifiers::NONE);
        assert!(
            matches!(&b.intent,Some(Intent::OpenFileDocument(path))if path=="/fixture/readme.md")
        );
        b.click(p, PointerButton::Secondary, Modifiers::NONE);
        let item = b.point("编辑 Markdown");
        b.click(item, PointerButton::Primary, Modifiers::NONE);
        assert!(
            matches!(&b.intent,Some(Intent::OpenFileDocument(path))if path=="/fixture/readme.md")
        );
    }
}

#[test]
fn file_view_submenu_aligns_with_other_rows_and_switches_the_actual_browser() {
    let mut b = Browser::new("S3", "LIST");
    let point = b.point("readme.md");
    b.click(point, PointerButton::Secondary, Modifiers::NONE);
    let out = b.frame(vec![]);
    let pos = |label| {
        out.shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => Some(t.pos),
                _ => None,
            })
            .unwrap()
    };
    let view = pos("视图");
    assert_eq!(view.x, pos("打开").x);
    assert_eq!(
        view.y - pos("重命名").y,
        crate::compact_menu::ROW_HEIGHT + 3.
    );
    assert_eq!(pos("刷新").y - view.y, crate::compact_menu::ROW_HEIGHT + 3.);
    let point = b.point("视图");
    b.click(point, PointerButton::Primary, Modifiers::NONE);
    let point = b.point("列视图");
    b.click(point, PointerButton::Primary, Modifiers::NONE);
    assert_eq!(b.view.file_view, "COLUMN");
}

#[test]
fn column_scrolling_is_independent_and_bucket_root_cannot_create_an_accidental_bucket() {
    let mut b = Browser::new("SFTP", "COLUMN");
    let pane = b.view.panes.get_mut(&Section::Files).unwrap();
    let entries = |prefix: &str| {
        (0..150)
            .map(|i| tomato_services::files::operations::Entry {
                name: format!("{prefix}-{i:03}.txt"),
                path: format!("/{prefix}/{i}"),
                directory: false,
                symlink: false,
                size: 1,
                modified: 0,
                permissions: 0,
            })
            .collect()
    };
    pane.columns.push(("/parent".into(), entries("parent")));
    pane.entries = entries("child");
    let point = b.point("child-000.txt");
    b.frame(vec![
        Event::PointerMoved(point),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0., -600.),
            modifiers: Modifiers::NONE,
        },
    ]);
    for _ in 0..35 {
        b.frame(vec![]);
    }
    let labels = b.labels();
    assert!(labels.iter().any(|s| s == "parent-000.txt"));
    assert!(!labels.iter().any(|s| s == "child-000.txt"));
    let mut b = Browser::new("S3", "LIST");
    b.connection.set("bucket", "");
    b.view.panes.get_mut(&Section::Files).unwrap().path = "/".into();
    b.frame(vec![]);
    b.click(
        egui::pos2(1100., 650.),
        PointerButton::Secondary,
        Modifiers::NONE,
    );
    let point = b.point("新建目录");
    b.click(point, PointerButton::Primary, Modifiers::NONE);
    assert!(
        b.view
            .dialog
            .as_ref()
            .is_some_and(|d| matches!(d.kind, Form::Notice))
    );
    assert!(
        !b.view.busy(Section::Files),
        "root directory command must not turn into CreateBucket"
    );
}

#[test]
fn column_range_selection_after_wheel_scroll_matches_the_visible_files() {
    let mut b = Browser::new("SFTP", "COLUMN");
    b.view.panes.get_mut(&Section::Files).unwrap().entries = (0..150)
        .map(|i| tomato_services::files::operations::Entry {
            name: format!("scrolled-{i:03}.txt"),
            path: format!("/fixture/{i}"),
            directory: false,
            symlink: false,
            size: 1,
            modified: 0,
            permissions: 0,
        })
        .collect();
    let point = b.point("scrolled-000.txt");
    b.frame(vec![
        Event::PointerMoved(point),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0., -600.),
            modifiers: Modifiers::NONE,
        },
    ]);
    for _ in 0..35 {
        b.frame(vec![]);
    }
    let visible: Vec<_> = b
        .frame(vec![])
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape
                && let Some(index) = text
                    .galley
                    .job
                    .text
                    .strip_prefix("scrolled-")
                    .and_then(|s| s.strip_suffix(".txt"))
                    .and_then(|s| s.parse::<usize>().ok())
            {
                Some((index, text.pos + text.galley.size() / 2.))
            } else {
                None
            }
        })
        .collect();
    assert!(visible.len() > 6 && visible[0].0 > 0);
    let start = egui::pos2(900., visible[2].1.y);
    let end = visible[4].1;
    b.frame(vec![
        Event::PointerMoved(start),
        Event::PointerButton {
            pos: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    b.frame(vec![Event::PointerMoved(end)]);
    assert_eq!(
        b.view.panes[&Section::Files].selected,
        visible[2..=4].iter().map(|(index, _)| *index).collect(),
        "range selection must use the same scroll origin as the displayed files"
    );
    b.frame(vec![Event::PointerButton {
        pos: end,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    assert!(b.view.panes[&Section::Files].file_drag.is_none());
}

#[test]
fn file_create_form_uses_java_default_name_enter_and_escape_without_opening_the_selected_file() {
    let mut b = Browser::new("SFTP", "LIST");
    let point = b.point("readme.md");
    b.click(point, PointerButton::Secondary, Modifiers::NONE);
    let create = b.point("创建文件");
    b.click(create, PointerButton::Primary, Modifiers::NONE);
    b.frame(vec![]);
    assert_eq!(b.view.dialog.as_ref().unwrap().fields[0], "新文件.md");
    b.frame(vec![Event::Text("created.md".into())]);
    assert_eq!(b.view.dialog.as_ref().unwrap().fields[0], "created.md");
    b.key(Key::Enter, Modifiers::NONE);
    assert!(b.view.dialog.is_none());
    assert!(b.intent.is_none());
    assert!(b.view.busy(Section::Files));
    let mut b = Browser::new("SFTP", "LIST");
    let point = b.point("readme.md");
    b.click(point, PointerButton::Secondary, Modifiers::NONE);
    let create = b.point("创建文件");
    b.click(create, PointerButton::Primary, Modifiers::NONE);
    b.frame(vec![]);
    b.key(Key::Escape, Modifiers::NONE);
    assert!(b.view.dialog.is_none());
    assert!(!b.view.busy(Section::Files));
}

#[test]
fn pointer_shift_on_icons_is_additive_while_the_detail_list_uses_a_range() {
    for mode in ["ICON", "LIST"] {
        let mut b = Browser::new("SFTP", mode);
        let first = b.point("readme.md");
        b.click(first, PointerButton::Primary, Modifiers::NONE);
        let third = b.point("third.txt");
        b.click(third, PointerButton::Primary, Modifiers::SHIFT);
        assert_eq!(
            b.view.panes[&Section::Files].selected,
            if mode == "ICON" {
                [0, 2].into()
            } else {
                [0, 1, 2].into()
            }
        );
    }
}

#[test]
fn java_icon_padding_spacing_blue_selection_and_keyboard_share_the_real_grid() {
    let mut b = Browser::new("SFTP", "ICON");
    let template = b.view.panes[&Section::Files].entries[0].clone();
    b.view.panes.get_mut(&Section::Files).unwrap().entries = (0..36)
        .map(|i| {
            let mut entry = template.clone();
            entry.name = format!("row-{i:03}.txt");
            entry.path = format!("/fixture/{}", entry.name);
            entry
        })
        .collect();
    let first = b.point("row-000.txt");
    let second = b.point("row-001.txt");
    let bounds = b.view.panes[&Section::Files].content_bounds.unwrap();
    assert_eq!(first.x - bounds.left(), 12. + 45.);
    assert_eq!(second.x - first.x, 90. + 8.);
    b.click(first, PointerButton::Primary, Modifiers::NONE);
    let out = b.frame(vec![]);
    let selected = out
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == egui::Color32::from_rgb(231, 243, 255) => {
                Some(rect)
            }
            _ => None,
        })
        .expect("Java icon selection uses a pale blue box");
    assert_eq!(selected.rect.width(), 90.);
    assert_eq!(selected.stroke.color, egui::Color32::from_rgb(51, 153, 255));
    assert_eq!(selected.stroke.width, 1.);
    b.key(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(b.view.panes[&Section::Files].file_cursor, Some(12));
    assert_eq!(b.point("row-012.txt").x, first.x);
    b.key(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(b.view.panes[&Section::Files].file_cursor, Some(0));
}
