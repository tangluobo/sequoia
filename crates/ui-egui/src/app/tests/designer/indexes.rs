//! Real controls: inline names, anchored type/method menus, ordered fields, SQL preview.
use super::*;
use tomato_core::table_design::indexes::{Field, Index, Kind};
fn state<'a>(app: &'a mut TomatoApp, id: &str) -> &'a mut Draft {
    let Panel::Sql { workbench, .. } = &mut app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
    else {
        unreachable!()
    };
    workbench.designer.as_mut().unwrap()
}
fn labels(app: &mut TomatoApp, id: &str) -> Vec<(String, egui::Rect)> {
    let ctx = app.context.clone();
    let mut result = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            ..Default::default()
        },
        |_| draw(app, &ctx, id),
    );
    let labels = result
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                Some((
                    t.galley.job.text.clone(),
                    egui::Rect::from_min_size(t.pos, t.galley.size()),
                ))
            } else {
                None
            }
        })
        .collect();
    result.textures_delta.clear();
    labels
}
fn press_label(app: &mut TomatoApp, id: &str, label: &str) {
    let found = labels(app, id);
    let point = found
        .iter()
        .rev()
        .find(|(s, _)| s == label)
        .unwrap_or_else(|| panic!("missing {label}: {found:?}"))
        .1
        .center();
    click(app, id, point.x, point.y);
}
fn setup(kind: &str) -> anyhow::Result<(TomatoApp, String, tempfile::TempDir)> {
    let (mut app, id, dir) = super::types::fixture(kind)?;
    let ctx = app.context.clone();
    state(&mut app, &id).switch_page(&ctx, Page::Indexes);
    frame(&mut app, &id, vec![]);
    Ok((app, id, dir))
}
fn add(app: &mut TomatoApp, id: &str) {
    press_label(app, id, "添加索引");
    frame(
        app,
        id,
        vec![egui::Event::Text("ix_new".into()), key(egui::Key::Enter)],
    );
}
fn fields(app: &mut TomatoApp, id: &str) {
    click(app, id, 300., 140.);
    let positions = labels(app, id);
    let id_rect = positions.iter().rev().find(|(s, _)| s == "id").unwrap().1;
    let label_rect = positions
        .iter()
        .rev()
        .find(|(s, _)| s == "label")
        .unwrap()
        .1;
    assert!((id_rect.left() - label_rect.left()).abs() < 1.);
    assert!(((id_rect.center().y - label_rect.center().y).abs() - 30.).abs() < 1.);
    for control in ["长度", "ASC"] {
        let cells: Vec<_> = positions.iter().filter(|(s, _)| s == control).collect();
        assert_eq!(cells.len(), 2);
        assert!((cells[0].1.left() - cells[1].1.left()).abs() < 1.);
    }
    press_label(app, id, "label");
    press_label(app, id, "id");
    press_label(app, id, "下移");
    press_label(app, id, "确定");
}
#[test]
fn index_controls_edit_names_fields_types_methods_without_unique_column_or_height_change()
-> anyhow::Result<()> {
    let (mut app, id, dir) = setup("MYSQL")?;
    add(&mut app, &id);
    assert_eq!(draft(&app, &id).request.indexes[0].name, "ix_new");
    fields(&mut app, &id);
    assert_eq!(
        draft(&app, &id).request.indexes[0]
            .fields
            .iter()
            .map(|f| f.name.as_str())
            .collect::<Vec<_>>(),
        ["label", "id"]
    );
    click(&mut app, &id, 510., 140.);
    assert!(egui::Popup::is_any_open(&app.context));
    press_label(&mut app, &id, "UNIQUE");
    assert_eq!(draft(&app, &id).request.indexes[0].kind, Kind::Unique);
    click(&mut app, &id, 630., 140.);
    press_label(&mut app, &id, "HASH");
    assert_eq!(draft(&app, &id).request.indexes[0].method, "HASH");
    let text = labels(&mut app, &id);
    assert!(!text.iter().any(|(s, _)| s == "唯一"));
    assert!(
        text.iter()
            .any(|(s, r)| s == "ix_new" && (r.center().y - 140.).abs() < 2.)
    );
    let ctx = app.context.clone();
    state(&mut app, &id).switch_page(&ctx, Page::Sql);
    frame(&mut app, &id, vec![]);
    let sql = &draft(&app, &id).preview;
    assert!(sql.contains("UNIQUE INDEX"));
    assert!(sql.contains("USING HASH"));
    assert!(!dir.path().join("connections.json").exists());
    app.close(&id);
    Ok(())
}
#[test]
fn postgres_index_method_menu_is_dialect_specific_and_fields_cancel_is_lossless()
-> anyhow::Result<()> {
    let (mut app, id, _dir) = setup("POSTGRESQL")?;
    add(&mut app, &id);
    click(&mut app, &id, 630., 140.);
    let text = labels(&mut app, &id);
    assert!(text.iter().any(|(s, _)| s == "gin"));
    assert!(!text.iter().any(|(s, _)| s == "FULLTEXT"));
    press_label(&mut app, &id, "hash");
    click(&mut app, &id, 300., 140.);
    press_label(&mut app, &id, "label");
    press_label(&mut app, &id, "id");
    press_label(&mut app, &id, "确定");
    assert!(
        draft(&app, &id)
            .indexes
            .fields
            .as_ref()
            .is_some_and(|p| p.error.contains("单个字段"))
    );
    press_label(&mut app, &id, "取消");
    assert!(draft(&app, &id).request.indexes[0].fields.is_empty());
    app.close(&id);
    Ok(())
}
#[test]
fn index_multi_selection_and_delete_apply_only_to_selected_rows() -> anyhow::Result<()> {
    let (mut app, id, _dir) = setup("MYSQL")?;
    for n in 0..3 {
        state(&mut app, &id).request.indexes.push(Index {
            name: format!("ix_{n}"),
            fields: vec![Field {
                name: "id".into(),
                ..Default::default()
            }],
            method: "BTREE".into(),
            ..Default::default()
        });
    }
    click(&mut app, &id, 10., 140.);
    let point = egui::pos2(10., 200.);
    for pressed in [true, false] {
        frame(
            &mut app,
            &id,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
        );
    }
    assert_eq!(draft(&app, &id).indexes.selected, [true, true, true]);
    press_label(&mut app, &id, "删除");
    assert!(draft(&app, &id).request.indexes.is_empty());
    app.close(&id);
    Ok(())
}

/// Complete real controls → actor → database → dependent-tab refresh, called by both owned DB fixtures.
pub(super) fn roundtrip(app: &mut TomatoApp, id: &str, kind: &str) -> anyhow::Result<()> {
    click(app, id, 70., 45.);
    assert_eq!(draft(app, id).page, Page::Indexes);
    add(app, id);
    fields(app, id);
    click(app, id, 510., 140.);
    press_label(app, id, "UNIQUE");
    click(app, id, 340., 45.);
    assert!(draft(app, id).preview.contains("UNIQUE INDEX"));
    click(app, id, 38., 16.);
    wait(app)?;
    let saved = &draft(app, id).saved.as_ref().unwrap().definition.indexes;
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "ix_new");
    assert_eq!(saved[0].kind, Kind::Unique);
    assert!(!draft(app, id).dirty);
    click(app, id, 70., 45.);
    click(app, id, 80., 140.);
    frame(app, id, vec![key(egui::Key::F2)]);
    frame(
        app,
        id,
        vec![
            egui::Event::Text("ix_renamed".into()),
            key(egui::Key::Enter),
        ],
    );
    click(app, id, 340., 45.);
    assert!(draft(app, id).preview.contains("RENAME"));
    assert!(!draft(app, id).preview.contains("DROP"));
    click(app, id, 38., 16.);
    wait(app)?;
    assert_eq!(
        draft(app, id).saved.as_ref().unwrap().definition.indexes[0].name,
        "ix_renamed"
    );
    click(app, id, 70., 45.);
    click(app, id, 510., 140.);
    press_label(app, id, "NORMAL");
    if kind == "POSTGRESQL" {
        click(app, id, 300., 140.);
        press_label(app, id, "label");
        press_label(app, id, "确定");
        click(app, id, 630., 140.);
        press_label(app, id, "hash");
    }
    click(app, id, 38., 16.);
    wait(app)?;
    let saved = &draft(app, id).saved.as_ref().unwrap().definition.indexes[0];
    assert_eq!(saved.kind, Kind::Normal);
    assert_eq!(
        saved.method,
        if kind == "POSTGRESQL" {
            "hash"
        } else {
            "BTREE"
        }
    );
    click(app, id, 10., 140.);
    press_label(app, id, "删除");
    click(app, id, 340., 45.);
    assert!(draft(app, id).preview.contains("DROP INDEX"));
    click(app, id, 38., 16.);
    wait(app)?;
    assert!(
        draft(app, id)
            .saved
            .as_ref()
            .unwrap()
            .definition
            .indexes
            .is_empty()
    );
    assert!(!draft(app, id).dirty);
    click(app, id, 24., 45.);
    Ok(())
}

#[test]
fn opaque_index_name_edit_preserves_signature_and_blocks_simplifying_fields() -> anyhow::Result<()>
{
    let (mut app, id, _dir) = setup("POSTGRESQL")?;
    let original = Index {
        name: "ix_special".into(),
        origin: Some("ix_special".into()),
        signature: "expression WHERE INCLUDE".into(),
        opaque: "special index".into(),
        method: "btree".into(),
        fields: vec![Field {
            name: "label".into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    state(&mut app, &id).request.indexes = vec![original.clone()];
    state(&mut app, &id)
        .saved
        .as_mut()
        .unwrap()
        .definition
        .indexes = vec![original];
    click(&mut app, &id, 300., 140.);
    assert!(draft(&app, &id).indexes.fields.is_none());
    click(&mut app, &id, 80., 140.);
    frame(&mut app, &id, vec![key(egui::Key::F2)]);
    frame(
        &mut app,
        &id,
        vec![egui::Event::Text("ix_safe".into()), key(egui::Key::Enter)],
    );
    let ctx = app.context.clone();
    state(&mut app, &id).switch_page(&ctx, Page::Sql);
    frame(&mut app, &id, vec![]);
    assert!(draft(&app, &id).preview.contains("RENAME TO \"ix_safe\""));
    assert!(!draft(&app, &id).preview.contains("DROP"));
    assert_eq!(
        draft(&app, &id).request.indexes[0].signature,
        "expression WHERE INCLUDE"
    );
    app.close(&id);
    Ok(())
}
#[test]
fn index_row_selector_hold_drag_selects_the_complete_range() -> anyhow::Result<()> {
    let (mut app, id, _dir) = setup("MYSQL")?;
    for n in 0..4 {
        state(&mut app, &id).request.indexes.push(Index {
            name: format!("ix_{n}"),
            fields: vec![Field {
                name: "id".into(),
                ..Default::default()
            }],
            method: "BTREE".into(),
            ..Default::default()
        });
    }
    frame(&mut app, &id, vec![]);
    let start = egui::pos2(10., 140.);
    let end = egui::pos2(10., 200.);
    frame(
        &mut app,
        &id,
        vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    for _ in 0..30 {
        frame(&mut app, &id, vec![]);
    }
    frame(&mut app, &id, vec![egui::Event::PointerMoved(end)]);
    frame(
        &mut app,
        &id,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert_eq!(draft(&app, &id).indexes.selected, [true, true, true, false]);
    app.close(&id);
    Ok(())
}

#[test]
fn index_field_dropdown_is_anchored_live_and_supports_nested_order_and_escape() -> anyhow::Result<()>
{
    let (mut app, id, _dir) = setup("MYSQL")?;
    add(&mut app, &id);
    click(&mut app, &id, 300., 140.);
    let rect = draft(&app, &id)
        .indexes
        .fields
        .as_ref()
        .unwrap()
        .popup
        .unwrap();
    assert!(
        (rect.left() - 202.).abs() < 2.
            && (rect.top() - 155.).abs() < 2.
            && rect.width() <= 450.
            && rect.height() <= 240.,
        "{rect:?}"
    );
    press_label(&mut app, &id, "label");
    assert_eq!(draft(&app, &id).request.indexes[0].fields[0].name, "label");
    press_label(&mut app, &id, "ASC");
    assert!(draft(&app, &id).indexes.fields.is_some());
    press_label(&mut app, &id, "DESC");
    assert!(draft(&app, &id).indexes.fields.is_some());
    assert!(draft(&app, &id).request.indexes[0].fields[0].descending);
    click(&mut app, &id, 830., 400.);
    assert!(draft(&app, &id).indexes.fields.is_none());
    assert!(!egui::Popup::is_any_open(&app.context));
    // Clicking the owning cell toggles the dropdown, and one outside click
    // dismisses both the field list and a nested direction menu.
    click(&mut app, &id, 300., 140.);
    click(&mut app, &id, 300., 140.);
    assert!(draft(&app, &id).indexes.fields.is_none());
    click(&mut app, &id, 300., 140.);
    press_label(&mut app, &id, "DESC");
    assert!(egui::Popup::is_any_open(&app.context));
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    assert!(draft(&app, &id).indexes.fields.is_some());
    assert!(!egui::Popup::is_any_open(&app.context));
    assert!(draft(&app, &id).request.indexes[0].fields[0].descending);
    press_label(&mut app, &id, "DESC");
    assert!(egui::Popup::is_any_open(&app.context));
    click(&mut app, &id, 830., 400.);
    assert!(draft(&app, &id).indexes.fields.is_none());
    assert!(!egui::Popup::is_any_open(&app.context));
    click(&mut app, &id, 300., 140.);
    press_label(&mut app, &id, "label");
    assert!(draft(&app, &id).request.indexes[0].fields.is_empty());
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    assert!(draft(&app, &id).indexes.fields.is_none());
    assert_eq!(draft(&app, &id).request.indexes[0].fields[0].name, "label");
    assert!(draft(&app, &id).request.indexes[0].fields[0].descending);
    click(&mut app, &id, 300., 140.);
    press_label(&mut app, &id, "id");
    click(&mut app, &id, 340., 45.);
    assert_eq!(draft(&app, &id).page, Page::Sql);
    assert!(draft(&app, &id).indexes.fields.is_none());
    assert!(!egui::Popup::is_any_open(&app.context));
    assert!(draft(&app, &id).preview.contains("DESC"));
    app.close(&id);
    Ok(())
}
