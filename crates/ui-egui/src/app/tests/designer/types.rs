//! Real grid, popup and TextEdit input; no database or saved configuration.
use super::*;
use crate::tabs::table_design::{Cell, type_picker::Picker};
use tomato_core::table_design::{Column, Dialect, Metadata, alter::Snapshot};
mod geometry;
mod scroll;
mod selection;

pub(super) fn fixture(kind: &str) -> anyhow::Result<(TomatoApp, String, tempfile::TempDir)> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let mut tab = tabs::create_tab(Connection::new(kind), None)?;
    let mut state = Draft::new(kind, "owned".into())?;
    state.request.table.name = "items".into();
    state.request.columns = vec![
        Column {
            name: "id".into(),
            data_type: if kind == "POSTGRESQL" {
                "integer"
            } else {
                "INT"
            }
            .into(),
            ..Default::default()
        },
        Column {
            name: "label".into(),
            data_type: if kind == "POSTGRESQL" {
                "character varying"
            } else {
                "VARCHAR"
            }
            .into(),
            length: "255".into(),
            ..Default::default()
        },
    ];
    state.load(Box::new(Snapshot {
        definition: state.request.clone(),
        extra_options: Default::default(),
        backslash_escapes: true,
        raw_columns: vec![
            "`id` int DEFAULT NULL".into(),
            "`label` varchar(255) DEFAULT NULL".into(),
        ],
        indexes: Default::default(),
        foreign_keys: Default::default(),
        triggers: Default::default(),
        sql: "CREATE TABLE items(id INT,label VARCHAR(255))".into(),
        view: false,
        primary_name: None,
    }));
    state.receive_metadata(std::sync::Arc::new(Metadata::empty(Dialect::resolve(
        kind,
    )?)));
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    workbench.designer = Some(Box::new(state));
    let id = tab.id.clone();
    app.active = Some(id.clone());
    app.tabs.push(tab);
    for _ in 0..3 {
        frame(&mut app, &id, vec![]);
    }
    Ok((app, id, directory))
}
fn picker<'a>(app: &'a TomatoApp, id: &str) -> &'a Picker {
    draft(app, id)
        .edit
        .as_ref()
        .unwrap()
        .type_picker
        .as_ref()
        .unwrap()
}
fn popup(app: &TomatoApp, id: &str) -> egui::Rect {
    let id = picker(app, id).popup.unwrap();
    app.context
        .memory(|memory| memory.area_rect(id))
        .expect("visible popup")
}
fn replace(app: &mut TomatoApp, id: &str, value: &str) {
    click(app, id, 205., 103.);
    let mut select_all = key(egui::Key::A);
    if let egui::Event::Key { modifiers, .. } = &mut select_all {
        *modifiers = egui::Modifiers {
            command: true,
            ..egui::Modifiers::CTRL
        };
    }
    frame(app, id, vec![select_all]);
    frame(app, id, vec![egui::Event::Text(value.into())]);
    frame(app, id, vec![]);
}

#[test]
fn type_press_opens_current_candidate_arrows_do_not_commit_and_escape_cancels() -> anyhow::Result<()>
{
    let (mut app, id, _directory) = fixture("MYSQL")?;
    let revision = draft(&app, &id).revision;
    let point = egui::pos2(205., 103.);
    frame(
        &mut app,
        &id,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::CTRL,
            },
        ],
    );
    assert_eq!(
        draft(&app, &id).edit.as_ref().unwrap().cell,
        Cell { row: 0, column: 1 }
    );
    frame(
        &mut app,
        &id,
        vec![egui::Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    frame(&mut app, &id, vec![]);
    let types = draft(&app, &id).types();
    assert_eq!(types[picker(&app, &id).focused.unwrap()], "INT");
    let rect = popup(&app, &id);
    assert!((rect.left() - 167.).abs() < 1. && (rect.top() - 120.).abs() < 1.);
    assert!((rect.width() - 135.).abs() < 1., "popup width: {rect:?}");
    assert!(rect.height() <= 505.);
    assert!(draft(&app, &id).selected(0, 1));
    assert!(!draft(&app, &id).selected(0, 0));
    assert!(!app.tabs.iter().find(|tab| tab.id == id).unwrap().dirty());
    frame(&mut app, &id, vec![key(egui::Key::ArrowDown)]);
    assert_ne!(types[picker(&app, &id).focused.unwrap()], "INT");
    assert_eq!(draft(&app, &id).request.columns[0].data_type, "INT");
    assert_eq!(draft(&app, &id).revision, revision);
    assert!(!app.tabs.iter().find(|tab| tab.id == id).unwrap().dirty());
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    assert!(draft(&app, &id).edit.is_none());
    assert!(!egui::Popup::is_any_open(&app.context));
    assert!(!draft(&app, &id).dirty);
    click(&mut app, &id, 205., 103.);
    frame(&mut app, &id, vec![key(egui::Key::ArrowDown)]);
    let chosen = types[picker(&app, &id).focused.unwrap()];
    frame(&mut app, &id, vec![key(egui::Key::Enter)]);
    assert!(draft(&app, &id).edit.is_none());
    assert_eq!(draft(&app, &id).request.columns[0].data_type, chosen);
    assert_eq!(draft(&app, &id).revision, revision + 1);
    assert!(draft(&app, &id).dirty);
    assert!(!egui::Popup::is_any_open(&app.context));
    Ok(())
}

#[test]
fn type_filter_mouse_choice_does_not_click_through_and_reopen_resets_filter() -> anyhow::Result<()>
{
    let (mut app, id, _directory) = fixture("MYSQL")?;
    click(&mut app, &id, 205., 103.);
    replace(&mut app, &id, "  chAr  ");
    assert_eq!(draft(&app, &id).edit.as_ref().unwrap().text, "  chAr  ");
    assert!(app.tabs.iter().find(|tab| tab.id == id).unwrap().dirty());
    assert_eq!(
        draft(&app, &id)
            .types()
            .iter()
            .filter(|value| picker(&app, &id).matches(value))
            .count(),
        2
    );
    let rect = popup(&app, &id);
    click(&mut app, &id, rect.left() + 20., rect.top() + 13.);
    assert!(draft(&app, &id).edit.is_none());
    assert_eq!(draft(&app, &id).request.columns[0].data_type, "CHAR");
    assert_eq!(draft(&app, &id).request.columns[1].data_type, "VARCHAR");
    assert_eq!(draft(&app, &id).focus, Some(Cell { row: 0, column: 1 }));
    click(&mut app, &id, 205., 103.);
    assert!(picker(&app, &id).matches("BIGINT"));
    assert_eq!(
        draft(&app, &id).types()[picker(&app, &id).focused.unwrap()],
        "CHAR"
    );
    replace(&mut app, &id, "int");
    let revision = draft(&app, &id).revision;
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    assert_eq!(draft(&app, &id).request.columns[0].data_type, "CHAR");
    assert_eq!(draft(&app, &id).revision, revision);
    click(&mut app, &id, 205., 103.);
    replace(&mut app, &id, "CUSTOM_TYPE");
    assert!(picker(&app, &id).focused.is_none());
    click(&mut app, &id, 760., 380.);
    assert_eq!(draft(&app, &id).request.columns[0].data_type, "CUSTOM_TYPE");
    assert!(draft(&app, &id).edit.is_none());
    assert!(!egui::Popup::is_any_open(&app.context));
    Ok(())
}

#[test]
fn type_current_candidate_scrolls_into_view_and_keyboard_choice_reaches_sql_preview()
-> anyhow::Result<()> {
    let (mut app, id, _directory) = fixture("MYSQL")?;
    click(&mut app, &id, 205., 133.);
    let rect = popup(&app, &id);
    assert!(
        (rect.left() - 167.).abs() < 1. && (rect.top() - 150.).abs() < 1.,
        "popup must stay directly below its cell: {rect:?}"
    );
    assert_eq!(
        draft(&app, &id).types()[picker(&app, &id).focused.unwrap()],
        "VARCHAR"
    );
    let ctx = app.context.clone();
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            ..Default::default()
        },
        |_| draw(&mut app, &ctx, &id),
    );
    assert!(
        output
            .shapes
            .iter()
            .any(|shape| matches!(&shape.shape, egui::Shape::Text(text)
        if text.galley.job.text == "VARCHAR" && rect.contains(text.pos))),
        "offscreen current type must be painted in popup {rect:?}: {:?}",
        output
            .shapes
            .iter()
            .filter_map(|shape| if let egui::Shape::Text(text) = &shape.shape {
                Some((&text.galley.job.text, text.pos))
            } else {
                None
            })
            .collect::<Vec<_>>()
    );
    for _ in 0..5 {
        frame(&mut app, &id, vec![key(egui::Key::ArrowDown)]);
    }
    assert_eq!(
        draft(&app, &id).types()[picker(&app, &id).focused.unwrap()],
        "YEAR"
    );
    assert_eq!(draft(&app, &id).request.columns[1].data_type, "VARCHAR");
    frame(&mut app, &id, vec![key(egui::Key::Enter)]);
    assert_eq!(draft(&app, &id).request.columns[1].data_type, "YEAR");
    click(&mut app, &id, 342., 45.);
    assert_eq!(draft(&app, &id).page, Page::Sql);
    assert!(draft(&app, &id).preview.contains("YEAR"));
    assert!(!egui::Popup::is_any_open(&app.context));
    Ok(())
}

#[test]
fn postgres_long_type_labels_fit_the_popup_without_wrapping() -> anyhow::Result<()> {
    let (mut app, id, _directory) = fixture("POSTGRESQL")?;
    let Panel::Sql { workbench, .. } =
        &mut app.tabs.iter_mut().find(|tab| tab.id == id).unwrap().panel
    else {
        unreachable!()
    };
    let mut metadata = Metadata::empty(Dialect::Postgres);
    metadata.data_types = &[
        "integer",
        "timestamp without time zone",
        "character varying",
    ];
    workbench
        .designer
        .as_mut()
        .unwrap()
        .receive_metadata(std::sync::Arc::new(metadata));
    click(&mut app, &id, 205., 103.);
    let rect = popup(&app, &id);
    assert!(rect.width() > 135. && rect.width() < 360.);
    assert!(picker(&app, &id).label_width.unwrap() + 35. <= rect.width() + 1.);
    assert_eq!(
        draft(&app, &id).types()[picker(&app, &id).focused.unwrap()],
        "integer"
    );
    assert!(!app.tabs.iter().find(|tab| tab.id == id).unwrap().dirty());
    frame(&mut app, &id, vec![key(egui::Key::Escape)]);
    Ok(())
}
