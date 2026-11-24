use super::*;
use tomato_services::terminal::ssh::tools::Table;
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    let proportional = fonts.families[&egui::FontFamily::Proportional].clone();
    fonts
        .families
        .insert(egui::FontFamily::Name("java-ui".into()), proportional);
    crate::font_assets::install(&ctx, fonts);
    crate::theme::install(&ctx);
    ctx
}
fn workspace() -> Workspace {
    let mut w = Workspace::default();
    w.standalone = Some(Section::Storage);
    w.visible.insert(Section::Storage);
    w.storage.replace("/dev/sdb1||1024|1024\n/dev/sdc1|data|2048|0\n__TOMATO_VGS__\ndata|2048|0\n__TOMATO_LVS__\n/dev/data/lv|data|lv|2048\n");
    w.panes.entry(Section::Storage).or_default().table = Some(Table {
        section: Section::Storage,
        columns: [
            "设备",
            "类型",
            "容量",
            "分区表",
            "文件系统",
            "挂载点",
            "状态",
        ]
        .map(str::to_owned)
        .into(),
        rows: vec![vec![
            "/dev/sdb".into(),
            "disk".into(),
            "1.00 GiB".into(),
            String::new(),
            String::new(),
            String::new(),
            "未分区".into(),
        ]],
        detail: String::new(),
    });
    w
}
fn frame(ctx: &egui::Context, w: &mut Workspace, events: Vec<egui::Event>) -> egui::FullOutput {
    frame_width(ctx, w, events, 1000.)
}
fn frame_width(
    ctx: &egui::Context,
    w: &mut Workspace,
    mut events: Vec<egui::Event>,
    width: f32,
) -> egui::FullOutput {
    events.insert(0, egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 800.),
            )),
            focused: true,
            events,
            ..Default::default()
        },
        |ui| {
            render(ui, w);
            dialogs::render(ui, w);
        },
    );
    out.textures_delta.clear();
    out
}
#[test]
fn narrowing_the_storage_page_keeps_all_seven_columns_in_view_before_and_after_manual_resize() {
    for saved in [None, Some([220., 100., 110., 100., 120., 200., 132.])] {
        let ctx = context();
        let mut w = workspace();
        w.storage.widths = saved;
        frame_width(&ctx, &mut w, vec![], 700.);
        let out = frame_width(&ctx, &mut w, vec![], 700.);
        for label in [
            "设备",
            "类型",
            "容量",
            "分区表",
            "文件系统",
            "挂载点",
            "状态",
        ] {
            let p = point(&out, label);
            assert!((8. ..692.).contains(&p.x), "{label}: {p:?}");
        }
    }
}
#[test]
fn device_column_boundaries_match_original_java_wide_and_narrow_components() {
    for (width, expected) in [
        (896., [175, 79, 105, 85, 105, 194, 135]),
        (700., [136, 62, 81, 65, 81, 152, 105]),
    ] {
        let ctx = context();
        let mut w = workspace();
        frame_width(&ctx, &mut w, vec![], width);
        let out = frame_width(&ctx, &mut w, vec![], width);
        let center = point(&out, "设备");
        let mut edges = out
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::LineSegment { points, .. } = &s.shape {
                    ((points[0].x - points[1].x).abs() < 0.01
                        && (points[0].y - center.y + 12.).abs() < 0.01
                        && (points[1].y - center.y - 12.).abs() < 0.01)
                        .then_some(points[0].x + 0.5)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        edges.sort_by(f32::total_cmp);
        assert_eq!(edges.len(), 7);
        let mut previous = 9.;
        let measured = edges
            .into_iter()
            .map(|edge| {
                let value = (edge - previous).round() as i32;
                previous = edge;
                value
            })
            .collect::<Vec<_>>();
        assert_eq!(measured, expected, "actual original TableView at {width}px");
    }
}
fn point(out: &egui::FullOutput, text: &str) -> egui::Pos2 {
    out.shapes
        .iter()
        .find_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                (t.galley.job.text == text).then_some(t.pos + t.galley.size() / 2.)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("Missing {text}"))
}
fn key(ctx: &egui::Context, w: &mut Workspace, key: egui::Key) {
    frame(
        ctx,
        w,
        [true, false]
            .into_iter()
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            })
            .collect(),
    );
}
#[test]
fn dragging_a_device_column_resizes_the_last_column_without_sorting_or_changing_row_height() {
    let ctx = context();
    let mut w = workspace();
    w.storage.widths = Some([170., 75., 100., 80., 100., 190., 130.]);
    let out = frame(&ctx, &mut w, vec![]);
    let header = point(&out, "设备");
    let row_y = point(&out, "/dev/sdb").y;
    let start = header + egui::vec2(85., 0.);
    frame(&ctx, &mut w, vec![egui::Event::PointerMoved(start)]);
    frame(
        &ctx,
        &mut w,
        vec![egui::Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        }],
    );
    let end = start + egui::vec2(50., 0.);
    frame(&ctx, &mut w, vec![egui::Event::PointerMoved(end)]);
    frame(
        &ctx,
        &mut w,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let out = frame(&ctx, &mut w, vec![]);
    assert!((w.storage.widths.unwrap()[0] - 220.).abs() < 1.);
    assert!(w.storage.sort.is_none());
    assert!((point(&out, "/dev/sdb").y - row_y).abs() < 0.5);
    assert!(w.panes[&Section::Storage].selected.is_empty());
}
#[test]
fn sorted_device_selection_and_keyboard_enter_keep_the_original_device_identity() {
    let ctx = context();
    let mut w = workspace();
    let rows = &mut w
        .panes
        .get_mut(&Section::Storage)
        .unwrap()
        .table
        .as_mut()
        .unwrap()
        .rows;
    let mut second = rows[0].clone();
    second[0] = "/dev/aaa".into();
    rows.push(second);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "设备");
    click(&ctx, &mut w, p);
    assert_eq!(w.storage.order, [1, 0]);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "/dev/aaa");
    click(&ctx, &mut w, p);
    assert!(w.panes[&Section::Storage].selected.contains(&1));
    key(&ctx, &mut w, egui::Key::End);
    assert!(w.panes[&Section::Storage].selected.contains(&0));
    key(&ctx, &mut w, egui::Key::Enter);
    assert_eq!(w.dialog.as_ref().unwrap().fields[0], "/dev/sdb");
}
#[test]
fn lvm_keyboard_collapses_expands_and_opens_the_selected_volume() {
    let ctx = context();
    let mut w = workspace();
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "VG  data  （容量 2.00 KiB，空闲 0 B）");
    click(&ctx, &mut w, p);
    key(&ctx, &mut w, egui::Key::ArrowLeft);
    assert!(w.storage.collapsed.contains("vg:data"));
    key(&ctx, &mut w, egui::Key::ArrowRight);
    assert!(!w.storage.collapsed.contains("vg:data"));
    key(&ctx, &mut w, egui::Key::ArrowDown);
    key(&ctx, &mut w, egui::Key::ArrowDown);
    assert_eq!(w.storage.selected_node().unwrap().kind, Kind::Lv);
    key(&ctx, &mut w, egui::Key::Enter);
    assert_eq!(w.dialog.as_ref().unwrap().fields[0], "/dev/data/lv");
}
fn click(ctx: &egui::Context, w: &mut Workspace, p: egui::Pos2) -> egui::FullOutput {
    frame(ctx, w, vec![egui::Event::PointerMoved(p)]);
    let events = |pressed| {
        vec![
            egui::Event::PointerMoved(p),
            egui::Event::PointerButton {
                pos: p,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ]
    };
    frame(ctx, w, events(true));
    let out = frame(ctx, w, events(false));
    frame(ctx, w, vec![]);
    out
}
#[test]
fn selected_lvm_nodes_feed_the_actual_toolbar_and_cancel_does_not_execute() {
    let ctx = context();
    let mut w = workspace();
    let out = frame(&ctx, &mut w, vec![]);
    let lv = point(&out, "LV  lv  （容量 2.00 KiB）");
    click(&ctx, &mut w, lv);
    assert_eq!(w.storage.selected_node().unwrap().path, "/dev/data/lv");
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "扩展 LV");
    click(&ctx, &mut w, p);
    assert_eq!(w.dialog.as_ref().unwrap().fields, ["/dev/data/lv", "+10G"]);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "取消");
    click(&ctx, &mut w, p);
    assert!(w.dialog.is_none());
    assert!(!w.busy(Section::Storage));
    let out = frame(&ctx, &mut w, vec![]);
    let pv = point(&out, "PV  /dev/sdc1  （容量 2.00 KiB，空闲 0 B）");
    click(&ctx, &mut w, pv);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "创建/扩展 VG");
    click(&ctx, &mut w, p);
    assert_eq!(w.dialog.as_ref().unwrap().fields, ["/dev/sdc1", "data"]);
}
#[test]
fn mount_controls_choose_filesystem_and_persistence_and_require_confirmation() {
    let ctx = context();
    let mut w = workspace();
    w.panes
        .get_mut(&Section::Storage)
        .unwrap()
        .selected
        .insert(0);
    w.dialog = Some(draft(&w, Form::Mount));
    frame(&ctx, &mut w, vec![]);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "不格式化");
    click(&ctx, &mut w, p);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "xfs");
    click(&ctx, &mut w, p);
    assert!(
        w.dialog.as_ref().unwrap().confirm.is_none(),
        "Choosing an item must not submit the form"
    );
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "写入 /etc/fstab");
    click(&ctx, &mut w, p);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "确定");
    click(&ctx, &mut w, p);
    assert!(
        !w.busy(Section::Storage),
        "No write before confirming the command"
    );
    assert!(
        !w.panes.contains_key(&Section::Files),
        "Storage forms must not create or refresh a hidden SFTP pane"
    );
    let Some(Operation::Manage(ManagementAction::Mount {
        device,
        filesystem,
        persist,
        ..
    })) = &w.dialog.as_ref().unwrap().confirm
    else {
        panic!("Missing mount confirmation")
    };
    assert_eq!(device, "/dev/sdb");
    assert_eq!(filesystem.as_deref(), Some("xfs"));
    assert!(!persist);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "取消");
    click(&ctx, &mut w, p);
    assert!(w.dialog.is_none());
    assert!(!w.busy(Section::Storage));
}
#[test]
fn invalid_devices_and_busy_inventory_cannot_submit_a_storage_write() {
    let ctx = context();
    let mut w = workspace();
    w.dialog = Some(dialogs::storage_dialog(
        Form::InitializePv,
        "/dev/sdb; touch bad".into(),
    ));
    frame(&ctx, &mut w, vec![]);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "确定");
    click(&ctx, &mut w, p);
    assert!(w.dialog.as_ref().unwrap().confirm.is_none());
    assert!(!w.busy(Section::Storage));
    w.dialog = None;
    w.enqueue(Section::Storage, Operation::Query(Section::Storage));
    frame(&ctx, &mut w, vec![]);
    let out = frame(&ctx, &mut w, vec![]);
    let p = point(&out, "新硬盘初始化为 PV");
    click(&ctx, &mut w, p);
    assert!(w.dialog.is_none());
}
