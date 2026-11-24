//! Opening metadata must not execute a view or retain a second copy of its data.
use super::*;
use tomato_core::workbench::{DatabaseAction, TableId, TableInfo};

#[test]
fn structure_and_data_have_independent_tabs_and_structure_never_reads_rows() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let connection = Connection::new("SQLITE");
    app.store.connections.push(connection.clone());
    app.open_connection(connection.clone());
    drain(&mut app)?;
    let root = app.active.clone().unwrap();
    assert_eq!(
        super::super::shell::tab_icon(&app.tabs[0]),
        Some("images/connect/query.png")
    );
    app.submit(&root, Command::Sql { connection: connection.clone(),
        // PRAGMA can describe this view, while COUNT/SELECT fails with integer overflow.
        sql: "CREATE VIEW metadata_only AS SELECT abs(-9223372036854775808) AS value; CREATE TABLE writable(id INTEGER PRIMARY KEY, value TEXT); INSERT INTO writable VALUES (1,'saved')".into()
    }, Purpose::Normal);
    drain(&mut app)?;
    let view = TableInfo {
        table: TableId {
            schema: "main".into(),
            name: "metadata_only".into(),
        },
        view: true,
    };
    app.open_table(connection.clone(), view.clone(), true);
    assert_eq!(
        super::super::shell::tab_icon(app.tabs.last().unwrap()),
        Some("images/connect/view.png")
    );
    drain(&mut app)?;
    let structure_id = app.active.clone().unwrap();
    let structure = app.tabs.iter().find(|tab| tab.id == structure_id).unwrap();
    assert!(structure.error.is_none(), "{:?}", structure.error);
    assert!(matches!(&structure.panel, Panel::Sql { workbench, .. }
        if workbench.structure && workbench.definition.as_ref().is_some_and(|d| d.view) && workbench.page.is_none()));
    if let Panel::Sql { workbench, .. } = &mut app
        .tabs
        .iter_mut()
        .find(|t| t.id == structure_id)
        .unwrap()
        .panel
    {
        workbench.definition.as_mut().unwrap().columns.clear();
    }
    app.table_tab_action(
        &structure_id,
        super::super::table_config::Action::RefreshDesign,
    );
    assert!(
        !app.pending.is_empty(),
        "readonly structures must reload metadata without requesting rows"
    );
    drain(&mut app)?;
    assert!(
        matches!(&app.tabs.iter().find(|t|t.id==structure_id).unwrap().panel,Panel::Sql {workbench,..} if workbench.definition.as_ref().unwrap().columns.len()==1 && workbench.page.is_none())
    );
    app.open_table(connection.clone(), view.clone(), true);
    assert_eq!(app.active.as_deref(), Some(structure_id.as_str()));
    app.open_table(connection.clone(), view, false);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !app.pending.is_empty() {
        app.logic(&ctx);
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "background command timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let data = app
        .tabs
        .iter_mut()
        .find(|tab| Some(&tab.id) == app.active.as_ref())
        .unwrap();
    assert_ne!(data.id, structure_id);
    assert!(
        data.error.is_none(),
        "Errors must not add a banner above the table"
    );
    if let Panel::Sql { workbench, .. } = &mut data.panel {
        assert!(
            workbench
                .failure
                .as_ref()
                .is_some_and(|failure| failure.text.contains("overflow"))
        );
        workbench.failure = None;
    } else {
        unreachable!();
    }
    assert_eq!(
        super::super::shell::tab_icon(data),
        Some("images/connect/view.png")
    );
    assert!(
        app.tabs
            .iter()
            .find(|tab| tab.id == structure_id)
            .unwrap()
            .error
            .is_none()
    );

    let table = TableInfo {
        table: TableId {
            schema: "main".into(),
            name: "writable".into(),
        },
        view: false,
    };
    app.open_table(connection.clone(), table.clone(), false);
    drain(&mut app)?;
    let data_id = app.active.clone().unwrap();
    if let Panel::Sql { workbench, .. } = &mut app
        .tabs
        .iter_mut()
        .find(|tab| tab.id == data_id)
        .unwrap()
        .panel
    {
        workbench.inserted.push(vec![
            None,
            Some(tomato_core::workbench::Cell::Text("unsaved".into())),
        ]);
    }
    app.open_table(connection.clone(), table.clone(), true);
    assert_eq!(
        super::super::shell::tab_icon(app.tabs.last().unwrap()),
        Some("images/connect/table.png")
    );
    drain(&mut app)?;
    let structure_id = app.active.clone().unwrap();
    assert_ne!(data_id, structure_id);
    assert!(
        matches!(&app.tabs.iter().find(|tab|tab.id==data_id).unwrap().panel,
        Panel::Sql { workbench, .. } if workbench.dirty() && !workbench.structure && workbench.page.as_ref().is_some_and(|page|page.rows.len()==1))
    );
    app.open_table(connection.clone(), table, false);
    assert_eq!(app.active.as_deref(), Some(data_id.as_str()));
    // A metadata refresh uses the same actor without discarding the other tab's draft.
    app.submit(
        &structure_id,
        Command::Database {
            connection,
            action: DatabaseAction::Definition {
                table: TableId {
                    schema: "main".into(),
                    name: "writable".into(),
                },
            },
        },
        Purpose::Normal,
    );
    drain(&mut app)?;
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
