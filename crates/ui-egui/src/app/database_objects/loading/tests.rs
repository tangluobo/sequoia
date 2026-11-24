//! Controller/widget behavior is separate from the owned protocol roundtrips.
use super::*;
use tomato_core::database_objects::{Catalog, Field, Fields, Object};
use tomato_core::workbench::TableId;

fn fixture() -> anyhow::Result<(tempfile::TempDir, TomatoApp, String)> {
    let directory = tempfile::tempdir()?;
    let repository = ConnectionFileRepository::new(directory.path().join("connections.json"));
    repository.save(&repository.load()?)?;
    let context = egui::Context::default();
    crate::theme::install(&context);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&context, runtime, repository);
    let connection = Connection::new("SQLITE");
    let mut tab = tabs::create_tab(connection.clone(), app.runtime.as_ref().map(|r| r.handle()))?;
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    workbench.schema = "main".into();
    workbench.browser = Some(Box::default());
    workbench.receive(DatabaseOutput::ObjectHeaders(Catalog {
        objects: vec![Object {
            table: TableId {
                schema: "main".into(),
                name: "one".into(),
            },
            view: false,
            comment: String::new(),
            engine: String::new(),
            auto_increment: String::new(),
            update_time: String::new(),
            data_length: String::new(),
            rows: String::new(),
            fields: Vec::new(),
        }],
        foreign_keys: Vec::new(),
    }));
    let id = tab.id.clone();
    app.tabs.push(tab);
    app.active = Some(id.clone());
    app.store.connections.push(connection);
    Ok((directory, app, id))
}
fn browser<'a>(app: &'a TomatoApp, id: &str) -> &'a tabs::database_objects::window::Draft {
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|t| t.id == id).unwrap().panel else {
        unreachable!()
    };
    workbench.browser.as_ref().unwrap()
}
fn payload() -> anyhow::Result<Output> {
    Ok(Output::Database(DatabaseOutput::ObjectFields {
        schema: "main".into(),
        fields: vec![Fields {
            name: "one".into(),
            columns: vec![Field {
                name: "new_field".into(),
                comment: "search remark".into(),
            }],
        }],
    }))
}
fn frame(
    app: &mut TomatoApp,
    id: &str,
    time: &mut f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    *time += 0.03;
    let context = app.context.clone();
    app.logic(&context);
    app.active = Some(id.into());
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            time: Some(*time),
            events,
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    output
}
#[test]
fn full_queue_defers_fields_and_the_next_completion_resumes_without_a_busy_overlay()
-> anyhow::Result<()> {
    let (_directory, mut app, id) = fixture()?;
    let held = app.queue.hold_admission_for_test();
    app.load_browser_fields(&id);
    assert!(browser(&app, &id).fields_loading == FieldsLoading::NotRequested);
    assert!(browser(&app, &id).state.fields_pending);
    assert!(!app.tabs.iter().find(|t| t.id == id).unwrap().busy);
    assert!(app.pending.is_empty());
    drop(held);
    let noop = u64::MAX;
    app.pending.insert(
        noop,
        Pending {
            tab_id: "absent-test-tab".into(),
            purpose: Purpose::Normal,
        },
    );
    app.queue.deliver_for_test(
        noop,
        Ok(Output::Database(DatabaseOutput::Databases(Vec::new()))),
    );
    let context = app.context.clone();
    app.logic(&context);
    assert!(browser(&app, &id).fields_loading == FieldsLoading::Pending);
    assert!(
        app.pending
            .values()
            .any(|p| p.tab_id == id && matches!(p.purpose, Purpose::ObjectFields(_)))
    );
    assert!(!app.tabs.iter().find(|t| t.id == id).unwrap().busy);
    Ok(())
}
#[test]
fn displayed_headers_can_enter_inline_rename_and_keep_it_when_fields_arrive() -> anyhow::Result<()>
{
    let (_directory, mut app, id) = fixture()?;
    let stamp = {
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
        else {
            unreachable!()
        };
        let browser = workbench.browser.as_mut().unwrap();
        browser.fields_loading = FieldsLoading::Pending;
        Stamp::new(connection, &workbench.schema, browser.generation)
    };
    let mut time = 1.;
    let output = frame(&mut app, &id, &mut time, vec![]);
    let point = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "one" => {
                Some(text.pos + text.galley.size() / 2.)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("object header was not rendered before fields"))?;
    for pressed in [true, false] {
        frame(
            &mut app,
            &id,
            &mut time,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    frame(
        &mut app,
        &id,
        &mut time,
        vec![egui::Event::Key {
            key: egui::Key::F2,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert_eq!(browser(&app, &id).rename.as_ref().unwrap().text, "one");
    app.browser_fields_reply(&id, &stamp, payload());
    frame(&mut app, &id, &mut time, vec![]);
    let browser = browser(&app, &id);
    assert_eq!(browser.rename.as_ref().unwrap().text, "one");
    assert_eq!(
        browser.catalog.objects[0].fields[0].comment,
        "search remark"
    );
    assert!(browser.notice.is_none() && !browser.state.fields_pending);
    Ok(())
}

#[test]
fn refresh_rejects_old_field_success_and_failure_without_releasing_the_foreground_request()
-> anyhow::Result<()> {
    let (_directory, mut app, id) = fixture()?;
    let stamp = {
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
        else {
            unreachable!()
        };
        let browser = workbench.browser.as_mut().unwrap();
        browser.fields_loading = FieldsLoading::Pending;
        let stamp = Stamp::new(connection, &workbench.schema, browser.generation);
        browser.begin_headers();
        stamp
    };
    app.tabs.iter_mut().find(|t| t.id == id).unwrap().busy = true;
    app.browser_fields_reply(&id, &stamp, payload());
    app.browser_fields_reply(&id, &stamp, Err(anyhow::anyhow!("obsolete preload failed")));
    assert!(!app.accept_browser_header(&id, &stamp));
    let tab = app.tabs.iter().find(|t| t.id == id).unwrap();
    assert!(tab.busy && tab.error.is_none());
    let browser = browser(&app, &id);
    assert!(browser.header_pending && browser.fields_loading == FieldsLoading::NotRequested);
    assert!(browser.catalog.objects[0].fields.is_empty() && browser.notice.is_none());
    Ok(())
}

#[test]
fn changed_namespace_database_and_closed_tab_reject_old_preload_payloads() -> anyhow::Result<()> {
    let (_directory, mut app, id) = fixture()?;
    let stamp = {
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &mut app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
        else {
            unreachable!()
        };
        let browser = workbench.browser.as_mut().unwrap();
        browser.fields_loading = FieldsLoading::Pending;
        Stamp::new(connection, &workbench.schema, browser.generation)
    };
    if let Panel::Sql { workbench, .. } =
        &mut app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
    {
        workbench.schema = "another_namespace".into();
    }
    app.browser_fields_reply(&id, &stamp, payload());
    assert!(browser(&app, &id).catalog.objects[0].fields.is_empty());
    if let Panel::Sql {
        connection,
        workbench,
        ..
    } = &mut app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
    {
        workbench.schema = "main".into();
        connection.set("database", serde_json::json!("another-database.sqlite"));
    }
    app.browser_fields_reply(&id, &stamp, payload());
    assert!(browser(&app, &id).catalog.objects[0].fields.is_empty());
    app.close(&id);
    app.browser_fields_reply(&id, &stamp, payload());
    assert!(!app.accept_browser_header(&id, &stamp));
    assert!(app.tabs.is_empty());
    Ok(())
}
