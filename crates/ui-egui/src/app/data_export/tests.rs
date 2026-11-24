use super::*;
use crate::app::tests::native_bridge::Bridge;
use tomato_core::{
    data_export::Schema,
    workbench::{Cell, Column, TableId},
};
fn ready(ctx: &egui::Context) -> Draft {
    let columns = vec![Column {
        name: "id".into(),
        data_type: "INTEGER".into(),
        nullable: false,
        default: None,
        primary_key: 1,
        generated: false,
        auto_increment: false,
        binary: false,
        type_schema: String::new(),
        type_name: String::new(),
    }];
    Draft::new(
        Snapshot {
            schema: Schema {
                table: TableId {
                    schema: "main".into(),
                    name: "items".into(),
                },
                dialect: "SQLITE".into(),
                columns,
            },
            rows: vec![vec![Cell::Integer(1)]],
        },
        Connection::new("SQLITE"),
        String::new(),
        true,
        ctx,
    )
}
#[test]
fn native_scope_buttons_then_all_ten_radio_formats_and_back_next_cancel() -> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let directory = tempfile::tempdir()?;
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
    app.data_export = Some(ready(&ctx));
    bridge.frame(&ctx, Vec::new(), |ctx| app.data_export_dialog(ctx));
    let point = bridge.text("当前 1 条记录")?;
    bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
    assert!(!app.data_export.as_ref().unwrap().scope);
    assert!(!app.data_export.as_ref().unwrap().all);
    bridge.frame(&ctx, Vec::new(), |ctx| app.data_export_dialog(ctx));
    for format in tomato_core::data_export::Format::ALL {
        let point = bridge.text(format.label())?;
        bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
        assert_eq!(app.data_export.as_ref().unwrap().options.format, format);
    }
    let point = bridge.text("下一步 >")?;
    bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
    assert_eq!(app.data_export.as_ref().unwrap().step, 1);
    let point = bridge.text("下一步 >")?;
    bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
    assert_eq!(app.data_export.as_ref().unwrap().step, 1);
    assert!(app.data_export.as_ref().unwrap().error.is_some());
    app.data_export.as_mut().unwrap().path = directory
        .path()
        .join("current.json")
        .to_string_lossy()
        .into_owned();
    let point = bridge.text("下一步 >")?;
    bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
    assert_eq!(app.data_export.as_ref().unwrap().step, 2);
    let point = bridge.text("< 上一步")?;
    bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
    assert_eq!(app.data_export.as_ref().unwrap().step, 1);
    let point = bridge.text("取消")?;
    bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
    assert!(app.data_export.is_none());
    assert!(app.store.connections.is_empty());
    Ok(())
}
#[test]
fn native_wizard_runs_background_export_without_modifying_configuration() -> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let directory = tempfile::tempdir()?;
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
    let mut draft = ready(&ctx);
    draft.scope = false;
    draft.step = 3;
    draft.options.format = tomato_core::data_export::Format::Json;
    draft.path = directory
        .path()
        .join("current.json")
        .to_string_lossy()
        .into_owned();
    app.data_export = Some(draft);
    bridge.frame(&ctx, Vec::new(), |ctx| app.data_export_dialog(ctx));
    let point = bridge.text("开始")?;
    bridge.click(&ctx, point, |ctx| app.data_export_dialog(ctx));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        bridge.frame(&ctx, Vec::new(), |ctx| app.data_export_dialog(ctx));
        let draft = app.data_export.as_ref().unwrap();
        // The worker can finish after this frame's poll. Its report becomes
        // observable when the next frame consumes the completion message.
        if draft.report.is_some() || draft.error.is_some() {
            assert!(draft.error.is_none(), "{:?}", draft.error);
            assert_eq!(draft.report.as_ref().unwrap().records, 1);
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let rows: serde_json::Value =
        serde_json::from_reader(std::fs::File::open(directory.path().join("current.json"))?)?;
    assert_eq!(rows[0]["id"], 1);
    Ok(())
}
#[test]
fn format_labels_share_one_left_edge_and_never_wrap() {
    let ctx = egui::Context::default();
    let mut draft = ready(&ctx);
    draft.scope = false;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640., 480.),
            )),
            ..Default::default()
        },
        |ui| {
            view::render(ui, &mut draft);
        },
    );
    output.textures_delta.clear();
    for format in tomato_core::data_export::Format::ALL {
        let text = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape
                    && text.galley.job.text == format.label()
                {
                    Some(text)
                } else {
                    None
                }
            })
            .expect("format label");
        assert_eq!(text.pos.x, 37.);
        assert_eq!(text.galley.rows.len(), 1);
    }
}

fn field_draft(ctx: &egui::Context, count: usize) -> Draft {
    let mut draft = ready(ctx);
    let snapshot = draft.current.as_mut().unwrap();
    let column = snapshot.schema.columns[0].clone();
    snapshot.schema.columns = (0..count)
        .map(|index| Column {
            name: match index {
                0 => "id".into(),
                1 => "department_id".into(),
                2 => "中文字段名称".into(),
                3 => format!("long_field_{}", "name_".repeat(40)),
                _ => format!("field_{index:04}"),
            },
            ..column.clone()
        })
        .collect();
    snapshot.rows = vec![vec![Cell::Integer(1); count]];
    draft.fields = vec![true; count];
    draft.field_order = (0..count).collect();
    draft.aliases = (0..count).map(|i| format!("FIELD{i}")).collect();
    draft.scope = false;
    draft.step = 2;
    draft
}

fn fields_frame(
    ctx: &egui::Context,
    draft: &mut Draft,
    time: &mut f64,
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    *time += 0.1;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640., 480.),
            )),
            time: Some(*time),
            events,
            ..Default::default()
        },
        |ui| {
            // Reproduce the large global row settings that caused the reported layout.
            ui.spacing_mut().interact_size.y = 40.;
            ui.spacing_mut().item_spacing.y = 18.;
            view::render(ui, draft);
        },
    );
    output.textures_delta.clear();
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some((
                text.galley.job.text.clone(),
                egui::Rect::from_min_size(text.pos, text.galley.size()),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn export_fields_align_left_keep_compact_rows_and_scroll_to_the_last_field() {
    let ctx = egui::Context::default();
    ctx.all_styles_mut(|style| style.animation_time = 0.);
    let mut draft = field_draft(&ctx, 256);
    let mut time = 0.;
    fields_frame(&ctx, &mut draft, &mut time, Vec::new());
    let text = fields_frame(&ctx, &mut draft, &mut time, Vec::new());
    let names = &draft.current.as_ref().unwrap().schema.columns;
    let rows = text
        .iter()
        .filter(|(name, _)| names.iter().any(|column| column.name == *name))
        .collect::<Vec<_>>();
    assert!(
        (12..20).contains(&rows.len()),
        "visible rows: {}",
        rows.len()
    );
    for pair in rows.windows(2) {
        assert!((pair[0].1.left() - pair[1].1.left()).abs() < 0.1);
        assert!((pair[1].1.top() - pair[0].1.top() - 22.).abs() < 0.1);
    }
    assert!(rows[0].1.left() < 60.);
    let long = rows
        .iter()
        .find(|(name, _)| name.starts_with("long_field_"))
        .unwrap();
    assert!(
        long.1.width() > 600.,
        "long name remains a single scrollable line"
    );
    let point = egui::pos2(300., 250.);
    fields_frame(
        &ctx,
        &mut draft,
        &mut time,
        vec![egui::Event::PointerMoved(point)],
    );
    fields_frame(
        &ctx,
        &mut draft,
        &mut time,
        vec![egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0., -100_000.),
            modifiers: Default::default(),
        }],
    );
    let mut text = Vec::new();
    for _ in 0..8 {
        text = fields_frame(&ctx, &mut draft, &mut time, Vec::new());
    }
    let last = text.iter().find(|(name, _)| name == "field_0255").unwrap();
    assert!(last.1.top() >= 120. && last.1.bottom() < 440.);
    let pos = last.1.center();
    for pressed in [true, false] {
        fields_frame(
            &ctx,
            &mut draft,
            &mut time,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert!(!draft.fields[255]);
    assert!(draft.fields[..255].iter().all(|selected| *selected));
    for (pos, selected) in [(egui::pos2(112., 96.), false), (egui::pos2(43., 96.), true)] {
        for pressed in [true, false] {
            fields_frame(
                &ctx,
                &mut draft,
                &mut time,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert!(draft.fields.iter().all(|value| *value == selected));
    }
}

#[test]
fn export_dbf_aliases_do_not_expand_field_rows_or_hide_the_bottom_navigation() {
    let ctx = egui::Context::default();
    let mut draft = field_draft(&ctx, 24);
    draft.options.format = tomato_core::data_export::Format::Dbase;
    let mut time = 0.;
    fields_frame(&ctx, &mut draft, &mut time, Vec::new());
    let text = fields_frame(&ctx, &mut draft, &mut time, Vec::new());
    let names = &draft.current.as_ref().unwrap().schema.columns;
    let rows = text
        .iter()
        .filter(|(name, _)| names.iter().any(|column| column.name == *name))
        .collect::<Vec<_>>();
    for pair in rows.windows(2) {
        assert!((pair[1].1.top() - pair[0].1.top() - 22.).abs() < 0.1);
    }
    assert!(
        text.iter()
            .any(|(name, rect)| name == "下一步 >" && rect.top() >= 448.)
    );
    assert_eq!(draft.aliases.len(), 24);
    let aliases = text
        .iter()
        .filter(|(name, _)| name.starts_with("FIELD"))
        .collect::<Vec<_>>();
    assert!(aliases.len() >= 10);
    for pair in aliases.windows(2) {
        assert!(
            (pair[0].1.left() - pair[1].1.left()).abs() < 0.1,
            "alias columns differ: {pair:?}"
        );
        assert!((pair[1].1.top() - pair[0].1.top() - 22.).abs() < 0.1);
    }
}
