//! Owned Oracle privilege boundary: fields open even when original DDL is denied.
use super::*;

fn disconnect(app: &mut TomatoApp, id: &str) -> anyhow::Result<()> {
    wait(app)?;
    let task = app.queue.submit(Command::Disconnect { id: id.into() })?;
    let until = Instant::now() + Duration::from_secs(60);
    loop {
        for result in app.queue.poll() {
            let requested = result.id == task;
            // Closing the last tab also queues an untracked disconnect. Both
            // must really finish; do not mistake that first reply for this one.
            result.result?;
            if requested {
                return Ok(());
            }
        }
        anyhow::ensure!(Instant::now() < until, "owned reader disconnect timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
pub(in super::super) fn exercise(
    app: &mut TomatoApp,
    owner: &Connection,
    system: &Connection,
    scope: &str,
    db: &mut tomato_services::Databases,
) -> anyhow::Result<()> {
    let name = format!("{scope}_R");
    let mut reader = owner.clone();
    reader.id = Connection::new("ORACLE").id;
    reader.set("username", name.as_str());
    reader.set("password", "OwnedFixture42");
    let table = tomato_core::workbench::TableId {
        schema: scope.into(),
        name: "lazy_acl".into(),
    };
    let target = format!("\"{scope}\".\"lazy_acl\"");
    let mut user_created = false;
    let mut table_created = false;
    let mut tab_id = None;
    let result = (|| -> anyhow::Result<()> {
        let runtime = app.runtime.as_ref().unwrap();
        runtime.block_on(db.query(
            system,
            &format!("CREATE USER \"{name}\" IDENTIFIED BY \"OwnedFixture42\""),
        ))?;
        user_created = true;
        runtime.block_on(db.query(system, &format!("GRANT CREATE SESSION TO \"{name}\"")))?;
        runtime.block_on(db.query(owner, &format!("CREATE TABLE {target} (\"id\" NUMBER)")))?;
        table_created = true;
        runtime.block_on(db.query(owner, &format!("GRANT SELECT ON {target} TO \"{name}\"")))?;
        app.store.connections.push(reader.clone());
        let mut host = crate::app::sql_tree::Host::new(reader.clone());
        host.schemas = std::sync::Arc::new(vec![scope.into()]);
        host.selected = scope.into();
        let info = tomato_core::workbench::TableInfo {
            table: table.clone(),
            view: false,
        };
        host.tables
            .insert(scope.into(), std::sync::Arc::new(vec![info.clone()]));
        app.sql_hosts.insert(reader.id.clone(), host);
        app.open_table(reader.clone(), info, true);
        let id = app
            .tabs
            .iter()
            .find_map(|tab| {
                if let Panel::Sql { connection, .. } = &tab.panel {
                    (connection.id == reader.id).then(|| tab.id.clone())
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow::anyhow!("reader designer did not open"))?;
        tab_id = Some(id.clone());
        wait(app)?;
        assert!(
            draft(app, &id).saved.is_some(),
            "SELECT-only reader must be able to open structure"
        );
        assert!(draft(app, &id).saved.as_ref().unwrap().sql.is_empty());
        assert_eq!(draft(app, &id).request.columns[0].name, "id");
        click(app, &id, 340., 45.);
        mode(app, &id, true)?;
        wait(app)?;
        frame(app, &id, vec![]);
        assert!(
            matches!(draft(app, &id).sql_definition, SqlDefinition::Failed(_)),
            "GET_DDL must remain subject to Oracle's actual privilege checks"
        );
        assert!(
            draft(app, &id).preview.contains("31603"),
            "{}",
            draft(app, &id).preview
        );
        assert!(
            draft(app, &id).notice.is_none(),
            "SQL preview errors retain the original in-place comment feedback"
        );
        for _ in 0..3 {
            frame(app, &id, vec![]);
        }
        assert!(
            app.pending.is_empty(),
            "denied DDL must not create an automatic retry loop"
        );
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(db.query(system, &format!("GRANT SELECT_CATALOG_ROLE TO \"{name}\"")))?;
        // A new login observes the new default role. This closes only the owned
        // reader's actor session, without modifying its connection or designer.
        disconnect(app, &reader.id)?;
        mode(app, &id, true)?;
        wait(app)?;
        frame(app, &id, vec![]);
        assert!(
            matches!(draft(app, &id).sql_definition, SqlDefinition::Ready),
            "{}",
            draft(app, &id).preview
        );
        let sql = copied(app, &id)?;
        assert!(sql.contains("CREATE TABLE") && sql.contains(&target));
        assert_eq!(sql, draft(app, &id).preview);
        Ok(())
    })();
    if let Some(id) = tab_id {
        app.close(&id);
    }
    let close = disconnect(app, &reader.id);
    app.sql_hosts.remove(&reader.id);
    app.store.connections.retain(|c| c.id != reader.id);
    let cleanup = app.runtime.as_ref().unwrap().block_on(async {
        db.disconnect(&reader.id).await?;
        if user_created {
            let sessions = db
                .query(
                    system,
                    &format!("SELECT COUNT(*) FROM v$session WHERE username='{name}'"),
                )
                .await?;
            anyhow::ensure!(
                sessions[0].rows[0][0] == "0",
                "owned reader disconnect returned before its Oracle sessions ended"
            );
            db.query(system, &format!("DROP USER \"{name}\" CASCADE"))
                .await?;
        }
        if table_created {
            db.query(owner, &format!("DROP TABLE {target}")).await?;
        }
        anyhow::Ok(())
    });
    result.and(close).and(cleanup)
}
