//! Actual widgets → designer save actor → owned database, using the existing isolated harness.
use super::*;
pub(super) fn labels(app: &mut TomatoApp, id: &str) -> Vec<(String, egui::Pos2)> {
    let ctx = app.context.clone();
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            ..Default::default()
        },
        |_| draw(app, &ctx, id),
    );
    output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some((t.galley.job.text.clone(), t.pos)),
            _ => None,
        })
        .collect()
}
pub(super) fn fill(app: &mut TomatoApp, id: &str, label: &str, value: &str) -> anyhow::Result<()> {
    let position = labels(app, id)
        .into_iter()
        .filter(|(text, _)| text == label)
        .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .map(|(_, pos)| pos)
        .ok_or_else(|| anyhow::anyhow!("missing detail field {label}"))?;
    click(app, id, position.x + 140., position.y + 8.);
    frame(
        app,
        id,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            },
            egui::Event::Text(value.into()),
        ],
    );
    frame(app, id, vec![]);
    Ok(())
}
#[test]
fn new_table_foreign_key_and_trigger_controls_generate_executable_preview() -> anyhow::Result<()> {
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
    let mut tab = tabs::create_tab(Connection::new("MYSQL"), None)?;
    let id = tab.id.clone();
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    let mut state = Draft::new("MYSQL", "owned".into())?;
    state.request.table.name = "items".into();
    state.request.columns[0].name = "id".into();
    state.request.columns[0].data_type = "INTEGER".into();
    workbench.designer = Some(Box::new(state));
    app.tabs.push(tab);
    click(&mut app, &id, 120., 45.);
    click(&mut app, &id, 40., 76.);
    assert_eq!(draft(&app, &id).page, Page::ForeignKeys);
    for (label, value) in [
        ("名称", "fk_ui"),
        ("字段", "id"),
        ("参考表", "parent"),
        ("参考字段", "id"),
    ] {
        fill(&mut app, &id, label, value)?;
    }
    assert_eq!(
        draft(&app, &id).info[1].rows.as_ref().unwrap()[0][0],
        "fk_ui"
    );
    click(&mut app, &id, 171., 45.);
    click(&mut app, &id, 50., 76.);
    for (label, value) in [("名称", "tr_ui"), ("语句", "BEGIN SET NEW.id=NEW.id; END")] {
        fill(&mut app, &id, label, value)?;
    }
    click(&mut app, &id, 340., 45.);
    let preview = &draft(&app, &id).preview;
    assert!(
        preview.contains("CONSTRAINT `fk_ui`")
            && preview.contains("CREATE TRIGGER `owned`.`tr_ui`"),
        "{preview}"
    );
    let page = draft(&app, &id).snapshot();
    assert_eq!(page.auxiliary.len(), 2);
    // Deleting a staged row must clear the detail editor and remove its DDL.
    click(&mut app, &id, 120., 45.);
    click(&mut app, &id, 130., 76.);
    assert!(draft(&app, &id).info[1].rows.as_ref().unwrap().is_empty());
    assert!(draft(&app, &id).info[1].form.is_none());
    app.close(&id);
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
pub(super) fn exercise(
    app: &mut TomatoApp,
    connection: &Connection,
    scope: &str,
    admin: &mut tomato_services::Databases,
) -> anyhow::Result<()> {
    let kind = connection.kind();
    let dialect = tomato_services::dialect::resolve(kind)?;
    let q = |s: &str| dialect.quote(s);
    let child = format!("{}.{}", q(scope)?, q("items")?);
    let parent = format!("{}.{}", q(scope)?, q("ui_parent")?);
    let function = format!("{}.{}", q(scope)?, q("ui_fill")?);
    let column = q("id")?;
    let setup = format!(
        "CREATE TABLE {parent} AS SELECT {column} FROM {child};ALTER TABLE {parent} ADD PRIMARY KEY({column})"
    );
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(admin.query(connection, &setup))?;
    if kind == "POSTGRESQL" {
        app.runtime.as_ref().unwrap().block_on(admin.query(connection,&format!("CREATE FUNCTION {function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.label := 'ui trigger'; RETURN NEW; END $$")))?;
    }
    sidebar_menu(app, "items", "设计表")?;
    let id = app.active.clone().unwrap();
    wait(app)?;
    if kind == "MYSQL" {
        click(app, &id, 228., 45.);
        for (label, value) in [
            ("平均行长:", "512"),
            ("行格式:", "Compact"),
            ("排序规则:", "utf8mb4_unicode_ci"),
        ] {
            fill(app, &id, label, value)?;
        }
        click(app, &id, 340., 45.);
        assert!(
            draft(app, &id).preview.contains("AVG_ROW_LENGTH=512")
                && draft(app, &id).preview.contains("ROW_FORMAT=Compact")
        );
        click(app, &id, 38., 16.);
        wait(app)?;
        assert!(
            !draft(app, &id).dirty,
            "{:?}",
            draft(app, &id).notice.as_ref().map(|n| &n.text)
        );
        let options = &draft(app, &id).saved.as_ref().unwrap().definition.options;
        assert_eq!(options.avg_row_length, "512");
        assert_eq!(options.row_format, "Compact");
        assert_eq!(options.collation, "utf8mb4_unicode_ci");
    }
    click(app, &id, 120., 45.);
    wait(app)?;
    click(app, &id, 40., 76.);
    for (label, value) in [
        ("名称", "fk_ui"),
        ("字段", "id"),
        ("参考表", "ui_parent"),
        ("参考字段", "id"),
    ] {
        fill(app, &id, label, value)?;
    }
    click(app, &id, 340., 45.);
    assert!(draft(app, &id).preview.contains("ADD CONSTRAINT"));
    click(app, &id, 38., 16.);
    wait(app)?;
    assert!(
        !draft(app, &id).dirty,
        "{:?}",
        draft(app, &id).notice.as_ref().map(|n| &n.text)
    );
    assert_eq!(
        draft(app, &id).saved.as_ref().unwrap().foreign_keys.rows[0][0],
        "fk_ui"
    );
    click(app, &id, 171., 45.);
    wait(app)?;
    click(app, &id, 50., 76.);
    let body = match kind {
        "MYSQL" => "BEGIN SET NEW.label='ui trigger'; END".into(),
        "ORACLE" => "BEGIN :NEW.\"label\" := 'ui trigger'; END;".into(),
        _ => format!("{function}()"),
    };
    for (label, value) in [("名称", "tr_ui"), ("语句", body.as_str())] {
        fill(app, &id, label, value)?;
    }
    click(app, &id, 340., 45.);
    assert!(draft(app, &id).preview.contains("CREATE TRIGGER"));
    if kind == "ORACLE" {
        assert!(draft(app, &id).preview.ends_with("END;\n/"));
    }
    click(app, &id, 38., 16.);
    wait(app)?;
    assert!(
        !draft(app, &id).dirty,
        "{:?}",
        draft(app, &id).notice.as_ref().map(|n| &n.text)
    );
    assert_eq!(
        draft(app, &id).saved.as_ref().unwrap().triggers.rows[0][0],
        "tr_ui"
    );
    app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!(
            "INSERT INTO {parent} VALUES(91);INSERT INTO {child}({column},{}) VALUES(91,'before')",
            q("label")?
        ),
    ))?;
    let rows = app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!("SELECT {} FROM {child} WHERE {column}=91", q("label")?),
    ))?;
    assert_eq!(rows[0].rows[0][0], "ui trigger");
    assert!(
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(admin.query(
                connection,
                &format!(
                    "INSERT INTO {child}({column},{}) VALUES(92,'blocked')",
                    q("label")?
                )
            ))
            .is_err()
    );
    for x in [120., 171.] {
        click(app, &id, x, 45.);
        wait(app)?;
        click(app, &id, 40., 140.);
        click(app, &id, 130., 76.);
        click(app, &id, 38., 16.);
        wait(app)?;
        assert!(!draft(app, &id).dirty);
    }
    let saved = draft(app, &id).saved.as_ref().unwrap();
    assert!(saved.triggers.rows.is_empty());
    app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!(
            "INSERT INTO {child}({column},{}) VALUES(92,'no trigger')",
            q("label")?
        ),
    ))?;
    let values = app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!("SELECT {} FROM {child} WHERE {column}=92", q("label")?),
    ))?;
    assert_eq!(values[0].rows[0][0], "no trigger");
    if kind == "POSTGRESQL" {
        click(app, &id, 24., 45.);
        click(app, &id, 80., 103.);
        let toggle = |app: &mut TomatoApp| -> anyhow::Result<()> {
            let pos = labels(app, &id)
                .into_iter()
                .find(|(text, _)| text == "自动递增")
                .map(|(_, p)| p)
                .ok_or_else(|| anyhow::anyhow!("missing identity checkbox"))?;
            click(app, &id, pos.x + 3., pos.y + 6.);
            click(app, &id, 38., 16.);
            wait(app)?;
            Ok(())
        };
        toggle(app)?;
        assert!(draft(app, &id).saved.as_ref().unwrap().definition.columns[0].auto_increment);
        app.runtime.as_ref().unwrap().block_on(admin.query(
            connection,
            &format!("INSERT INTO {child}(label) VALUES('ui identity')"),
        ))?;
        let row = app.runtime.as_ref().unwrap().block_on(admin.query(
            connection,
            &format!("SELECT id FROM {child} WHERE label='ui identity'"),
        ))?;
        assert_eq!(row[0].rows[0][0], "93");
        click(app, &id, 80., 103.);
        toggle(app)?;
        assert!(!draft(app, &id).saved.as_ref().unwrap().definition.columns[0].auto_increment);
    }
    app.runtime.as_ref().unwrap().block_on(admin.query(
        connection,
        &format!("DELETE FROM {child} WHERE {column}>=91;DROP TABLE {parent}"),
    ))?;
    if kind == "POSTGRESQL" {
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(admin.query(connection, &format!("DROP FUNCTION {function}()")))?;
    }
    app.close(&id);
    Ok(())
}
