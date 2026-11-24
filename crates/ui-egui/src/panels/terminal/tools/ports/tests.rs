use super::*;
use egui::{Event, Modifiers, PointerButton, Pos2};
struct View {
    ctx: egui::Context,
    workspace: Workspace,
    icons: crate::icons::Icons,
    connection: tomato_core::Connection,
}
impl View {
    fn new(count: usize) -> Self {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            egui::FontFamily::Name("java-ui".into()),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        crate::font_assets::install(&ctx, fonts);
        crate::theme::install(&ctx);
        let mut workspace = Workspace::default();
        workspace.standalone = Some(Section::Ports);
        workspace.visible.insert(Section::Ports);
        let pane = workspace.panes.entry(Section::Ports).or_default();
        pane.status = format!("共 {count} 项");
        pane.table = Some(tomato_services::terminal::ssh::tools::Table {
            section: Section::Ports,
            columns: ["协议", "本地地址", "端口", "PID", "进程"]
                .map(str::to_owned)
                .into(),
            rows: (0..count)
                .map(|i| {
                    vec![
                        "tcp".into(),
                        "[::]".into(),
                        (30000 + i).to_string(),
                        "42".into(),
                        format!("proc-{i:03}"),
                    ]
                })
                .collect(),
            detail: String::new(),
        });
        Self {
            ctx,
            workspace,
            icons: Default::default(),
            connection: tomato_core::Connection::new("SSH"),
        }
    }
    fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
        self.frame_width(events, 1000.)
    }
    fn frame_width(&mut self, events: Vec<Event>, width: f32) -> egui::FullOutput {
        let mut out = self.ctx.run_ui(
            egui::RawInput {
                focused: true,
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(width, 800.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                render(ui, &mut self.workspace, &self.connection, &mut self.icons);
            },
        );
        out.textures_delta.clear();
        out
    }
    fn point(out: &egui::FullOutput, label: &str) -> Pos2 {
        out.shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    (t.galley.job.text == label).then_some(t.pos + t.galley.size() / 2.)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| panic!("Missing {label}"))
    }
}
#[test]
fn standalone_ports_keep_header_and_footer_fixed_while_only_visible_rows_scroll() {
    let mut view = View::new(500);
    view.frame(vec![]);
    let out = view.frame(vec![]);
    let header = View::point(&out, "协议");
    assert!(
        (header.y - 65.).abs() < 1.,
        "Java PortPanel table starts at y=53 with a 24px header: {header:?}"
    );
    let footer = View::point(&out, "共 500 个监听端口（总计 500）");
    view.frame(vec![
        Event::PointerMoved(egui::pos2(700., 400.)),
        Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0., -950.),
            modifiers: Modifiers::NONE,
        },
    ]);
    for _ in 0..35 {
        view.frame(vec![]);
    }
    let out = view.frame(vec![]);
    assert_eq!(View::point(&out, "协议"), header);
    assert_eq!(View::point(&out, "共 500 个监听端口（总计 500）"), footer);
    let rows = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                t.galley
                    .job
                    .text
                    .starts_with("proc-")
                    .then_some((s.clip_rect, t))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(!rows.is_empty() && rows.len() < 40);
    assert!(!rows.iter().any(|(_, t)| t.galley.job.text == "proc-000"));
    assert!(
        rows.iter()
            .all(|(clip, _)| clip.top() > header.y && clip.bottom() < footer.y)
    );
}
#[test]
fn manager_column_drag_retains_row_height_and_does_not_select_a_row() {
    let mut view = View::new(3);
    view.workspace
        .panes
        .get_mut(&Section::Ports)
        .unwrap()
        .manager_widths = Some(vec![80., 200., 90., 70., 544.].into_boxed_slice());
    view.frame(vec![]);
    let out = view.frame(vec![]);
    let start = View::point(&out, "协议") + egui::vec2(40., 0.);
    let row = View::point(&out, "proc-000");
    view.frame(vec![Event::PointerMoved(start)]);
    view.frame(vec![Event::PointerButton {
        pos: start,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    }]);
    let end = start + egui::vec2(40., 0.);
    view.frame(vec![Event::PointerMoved(end)]);
    view.frame(vec![Event::PointerButton {
        pos: end,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    let out = view.frame(vec![]);
    let pane = &view.workspace.panes[&Section::Ports];
    assert!((pane.manager_widths.as_ref().unwrap()[0] - 120.).abs() < 1.);
    assert!(pane.selected.is_empty());
    assert_eq!(View::point(&out, "proc-000").y, row.y);
}
#[test]
fn protocol_filter_clears_old_selection_and_counts_only_visible_ports() {
    let mut view = View::new(2);
    view.workspace
        .panes
        .get_mut(&Section::Ports)
        .unwrap()
        .table
        .as_mut()
        .unwrap()
        .rows[1][0] = "udp".into();
    view.frame(vec![]);
    let out = view.frame(vec![]);
    View::point(&out, "共 1 个监听端口（总计 2）");
    let pane = view.workspace.panes.get_mut(&Section::Ports).unwrap();
    pane.selected.insert(0);
    pane.anchor = Some(0);
    for (label, expected) in [
        ("UDP", "共 2 个监听端口（总计 2）"),
        ("TCP", "共 1 个监听端口（总计 2）"),
    ] {
        let out = view.frame(vec![]);
        let p = View::point(&out, label);
        view.frame(vec![Event::PointerMoved(p)]);
        for pressed in [true, false] {
            view.frame(vec![
                Event::PointerMoved(p),
                Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
        let out = view.frame(vec![]);
        View::point(&out, expected);
        let pane = &view.workspace.panes[&Section::Ports];
        assert!(pane.selected.is_empty());
        assert!(pane.anchor.is_none());
    }
}
#[test]
fn a_narrowed_port_page_keeps_every_resized_column_visible() {
    let mut view = View::new(3);
    view.workspace
        .panes
        .get_mut(&Section::Ports)
        .unwrap()
        .manager_widths = Some(vec![200., 280., 100., 120., 284.].into_boxed_slice());
    view.frame(vec![]);
    let out = view.frame_width(vec![], 640.);
    for label in ["协议", "本地地址", "端口", "PID", "进程"] {
        let p = View::point(&out, label);
        assert!((0. ..640.).contains(&p.x), "{label}: {p:?}");
    }
}
