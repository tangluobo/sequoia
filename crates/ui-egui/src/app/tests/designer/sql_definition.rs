//! Actual preview widgets, quiet actor delivery, stale results, failure retry and copy.
use super::auxiliary::labels;
mod permissions;
use super::foreign_names::output;
use super::*;
use crate::tabs::table_design::SqlDefinition;
pub(super) use permissions::exercise as exercise_permissions;
use tomato_core::{
    table_design::{Dialect, Metadata, alter::Snapshot},
    workbench::DatabaseOutput,
};

pub(super) fn mode(app: &mut TomatoApp, id: &str, save_as: bool) -> anyhow::Result<()> {
    let caption = labels(app, id)
        .into_iter()
        .filter(|(text, p)| matches!(text.as_str(), "保存" | "另存为") && p.y > 500.)
        .map(|(_, p)| p)
        .next()
        .ok_or_else(|| anyhow::anyhow!("preview mode selector missing"))?;
    click(app, id, caption.x + 8., caption.y + 6.);
    let value = if save_as { "另存为" } else { "保存" };
    let option = labels(app, id)
        .into_iter()
        .find(|(text, p)| text == value && p.y > 100. && *p != caption)
        .map(|(_, p)| p)
        .ok_or_else(|| anyhow::anyhow!("preview option {value} missing"))?;
    click(app, id, option.x + 8., option.y + 6.);
    Ok(())
}
pub(super) fn copied(app: &mut TomatoApp, id: &str) -> anyhow::Result<String> {
    click(app, id, 225., 105.);
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    output(
        app,
        id,
        vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ],
    );
    let result = output(app, id, vec![egui::Event::Copy]);
    output(
        app,
        id,
        vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)],
    );
    result
        .platform_output
        .commands
        .into_iter()
        .find_map(|command| {
            if let egui::OutputCommand::CopyText(text) = command {
                Some(text)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("preview did not copy selected SQL"))
}
fn request(app: &TomatoApp, id: &str) -> anyhow::Result<(u64, u64)> {
    app.pending
        .iter()
        .find_map(|(task, p)| {
            if p.tab_id == id {
                if let Purpose::DesignSql(g) = p.purpose {
                    Some((*task, g))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("no pending SQL definition"))
}
fn snapshot() -> anyhow::Result<Snapshot> {
    let mut state = Draft::new("ORACLE", "OWNED".into())?;
    state.request.table.name = "items".into();
    state.request.columns[0].name = "id".into();
    state.request.columns[0].data_type = "NUMBER".into();
    Ok(Snapshot {
        definition: state.request,
        extra_options: Default::default(),
        backslash_escapes: false,
        raw_columns: vec!["signature".into()],
        indexes: Default::default(),
        foreign_keys: Default::default(),
        triggers: Default::default(),
        sql: String::new(),
        view: false,
        primary_name: None,
    })
}
#[test]
fn lazy_sql_preview_uses_widgets_retries_without_flooding_and_ignores_stale_deliveries()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("connections.json");
    let repository = ConnectionFileRepository::new(file.clone());
    repository.save(&repository.load()?)?;
    let original = std::fs::read(&file)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    // The current-thread actor stays parked; completions exercise the real UI
    // dispatcher without contacting a database. Real servers are tested below.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    let mut c = Connection::new("ORACLE");
    c.set("host", "127.0.0.1");
    c.set("port", 9);
    let mut tab = tabs::create_tab(c, None)?;
    let id = tab.id.clone();
    let mut state = Draft::new("ORACLE", "OWNED".into())?;
    state.load(Box::new(snapshot()?));
    state.receive_metadata(std::sync::Arc::new(Metadata::empty(Dialect::Oracle)));
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    workbench.designer = Some(Box::new(state));
    app.tabs.push(tab);
    app.active = Some(id.clone());
    click(&mut app, &id, 340., 45.);
    assert!(
        app.pending.is_empty(),
        "ordinary ALTER preview must not request original DDL"
    );
    mode(&mut app, &id, true)?;
    let (first, _) = request(&app, &id)?;
    assert!(
        !app.tabs[0].busy,
        "DDL preview must not block the field grid or draw a page spinner"
    );
    app.queue
        .deliver_for_test(first, Err(anyhow::anyhow!("catalog denied\nsecond line")));
    app.logic(&ctx);
    for _ in 0..3 {
        frame(&mut app, &id, vec![]);
    }
    assert!(
        app.pending.is_empty(),
        "a failed preview must not resubmit on every paint"
    );
    assert!(
        draft(&app, &id)
            .preview
            .lines()
            .all(|line| line.starts_with("--"))
    );
    mode(&mut app, &id, true)?;
    let (task, generation) = request(&app, &id)?;
    for _ in 0..3 {
        frame(&mut app, &id, vec![]);
    }
    assert_eq!(
        app.pending.len(),
        1,
        "loading preview requests must be coalesced"
    );
    click(&mut app, &id, 26., 45.);
    click(&mut app, &id, 110., 16.);
    assert!(
        draft(&app, &id).dirty && draft(&app, &id).request.columns.len() == 2,
        "field controls must remain usable while DDL loads"
    );
    click(&mut app, &id, 550., 16.);
    let refresh = *app
        .pending
        .iter()
        .find(|(_, p)| p.tab_id == id && matches!(p.purpose, Purpose::Normal))
        .unwrap()
        .0;
    let sql = "CREATE TABLE \"OWNED\".\"items\" (\"id\" NUMBER);\n-- complete definition";
    app.queue.deliver_for_test(
        task,
        Ok(Output::Database(DatabaseOutput::DesignSql {
            table: snapshot()?.definition.table,
            sql: sql.into(),
        })),
    );
    app.logic(&ctx);
    assert!(
        app.tabs[0].busy && draft(&app, &id).busy,
        "quiet DDL delivery must not clear a concurrent refresh"
    );
    app.queue.deliver_for_test(
        refresh,
        Ok(Output::Database(DatabaseOutput::Design(Box::new(
            snapshot()?,
        )))),
    );
    app.logic(&ctx);
    assert!(draft(&app, &id).saved.as_ref().unwrap().sql.is_empty());
    click(&mut app, &id, 340., 45.);
    let (current, current_generation) = request(&app, &id)?;
    assert_ne!(generation, current_generation);
    let stale = 100_000;
    app.pending.insert(
        stale,
        Pending {
            tab_id: id.clone(),
            purpose: Purpose::DesignSql(generation),
        },
    );
    app.queue.deliver_for_test(
        stale,
        Ok(Output::Database(DatabaseOutput::DesignSql {
            table: snapshot()?.definition.table,
            sql: "STALE".into(),
        })),
    );
    app.logic(&ctx);
    assert!(matches!(
        draft(&app, &id).sql_definition,
        SqlDefinition::Loading
    ));
    app.queue.deliver_for_test(
        current,
        Ok(Output::Database(DatabaseOutput::DesignSql {
            table: snapshot()?.definition.table,
            sql: sql.into(),
        })),
    );
    app.logic(&ctx);
    frame(&mut app, &id, vec![]);
    assert_eq!(draft(&app, &id).preview, sql);
    assert_eq!(
        copied(&mut app, &id)?,
        sql,
        "paint cache and clipboard must use the loaded SQL"
    );
    mode(&mut app, &id, false)?;
    assert!(!draft(&app, &id).preview_save_as);
    assert!(
        app.pending.is_empty(),
        "selecting the save mode must use local ALTER SQL"
    );
    assert_eq!(std::fs::read(file)?, original);
    Ok(())
}

pub(super) fn exercise(
    app: &mut TomatoApp,
    c: &Connection,
    scope: &str,
    db: &mut tomato_services::Databases,
) -> anyhow::Result<()> {
    let dialect = tomato_services::dialect::resolve(c.kind())?;
    let q = |s: &str| dialect.quote(s);
    let table = tomato_core::workbench::TableId {
        schema: scope.into(),
        name: "lazy_ui_items".into(),
    };
    let target = format!("{}.{}", q(scope)?, q(&table.name)?);
    let copied_target = format!("{}.{}", q(scope)?, q("lazy_ui_copy")?);
    let (number, text) = if c.kind() == "ORACLE" {
        ("NUMBER", "VARCHAR2(40)")
    } else {
        ("INTEGER", "VARCHAR(40)")
    };
    app.runtime.as_ref().unwrap().block_on(db.query(
        c,
        &format!(
            "CREATE TABLE {target} ({} {number},{} {text})",
            q("id")?,
            q("label")?
        ),
    ))?;
    let host = app.sql_hosts.get_mut(&c.id).ok_or_else(|| {
        anyhow::anyhow!("preview workflow requires its fixture's open connection")
    })?;
    let tables = host
        .tables
        .get_mut(scope)
        .ok_or_else(|| anyhow::anyhow!("preview workflow requires its fixture's loaded scope"))?;
    std::sync::Arc::make_mut(tables).push(tomato_core::workbench::TableInfo {
        table: table.clone(),
        view: false,
    });
    sidebar_menu(app, &table.name, "打开表")?;
    let sibling = app.active.clone().unwrap();
    wait(app)?;
    sidebar_menu(app, &table.name, "设计表")?;
    let id = app.active.clone().unwrap();
    wait(app)?;
    assert!(
        draft(app, &id).saved.as_ref().unwrap().sql.is_empty(),
        "opening fields must leave preview DDL unloaded"
    );
    click(app, &id, 340., 45.);
    assert!(
        app.pending.is_empty(),
        "ordinary ALTER preview must stay local"
    );
    mode(app, &id, true)?;
    assert!(!app.tabs.iter().find(|t| t.id == id).unwrap().busy);
    wait(app)?;
    frame(app, &id, vec![]);
    let before = copied(app, &id)?;
    assert_eq!(before, draft(app, &id).preview);
    assert!(before.contains("CREATE") && before.contains(&q("id")?));
    mode(app, &id, false)?;
    click(app, &id, 26., 45.);
    edit(app, &id, 80., 95., "renamed_id");
    click(app, &id, 340., 45.);
    assert!(app.pending.is_empty());
    assert!(
        draft(app, &id).preview.contains("ALTER") && draft(app, &id).preview.contains("renamed_id")
    );
    click(app, &id, 38., 16.);
    wait(app)?;
    assert!(
        !draft(app, &id).dirty,
        "{:?}",
        draft(app, &id).notice.as_ref().map(|n| &n.text)
    );
    assert!(
        draft(app, &id).saved.as_ref().unwrap().sql.is_empty(),
        "save/refresh must not eagerly retain another DDL copy"
    );
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|t| t.id == sibling).unwrap().panel
    else {
        unreachable!()
    };
    assert_eq!(
        workbench.page.as_ref().unwrap().definition.columns[0].name,
        "renamed_id"
    );
    mode(app, &id, true)?;
    wait(app)?;
    frame(app, &id, vec![]);
    let sql = copied(app, &id)?;
    assert_eq!(sql, draft(app, &id).preview);
    assert!(sql.contains("renamed_id"));
    let original = if sql.contains(&target) {
        target.clone()
    } else {
        q(&table.name)?
    };
    assert!(sql.contains(&original));
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(db.query(c, &sql.replace(&original, &copied_target)))?;
    app.runtime.as_ref().unwrap().block_on(db.query(
        c,
        &format!("INSERT INTO {copied_target} VALUES(7,'copied')"),
    ))?;
    let rows = app.runtime.as_ref().unwrap().block_on(db.query(
        c,
        &format!("SELECT {} FROM {copied_target}", q("renamed_id")?),
    ))?;
    assert_eq!(rows[0].rows[0][0], "7");
    app.close(&id);
    app.close(&sibling);
    app.runtime.as_ref().unwrap().block_on(db.query(
        c,
        &format!("DROP TABLE {copied_target};DROP TABLE {target}"),
    ))?;
    if let Some(tables) = app
        .sql_hosts
        .get_mut(&c.id)
        .and_then(|host| host.tables.get_mut(scope))
    {
        std::sync::Arc::make_mut(tables).retain(|item| item.table != table);
    }
    Ok(())
}
