use super::*;
#[test]
fn opened_database_and_schema_icons_survive_collapsing_the_branch_without_closing_it()
-> anyhow::Result<()> {
    for (kind, namespace, expected) in [
        ("MYSQL", "sample_db", "images/connect/database.png"),
        ("POSTGRESQL", "public", "images/connect/mod_open.png"),
    ] {
        let directory = tempfile::tempdir()?;
        let ctx = egui::Context::default();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let mut app = TomatoApp::with_repository(
            &ctx,
            runtime,
            ConnectionFileRepository::new(directory.path().join("connections.json")),
        );
        let connection = Connection::new(kind);
        app.store.connections.push(connection.clone());
        let mut host = crate::app::sql_tree::Host::new(connection.clone());
        host.schemas = std::sync::Arc::new(vec![namespace.into()]);
        host.tables
            .insert(namespace.into(), std::sync::Arc::new(Vec::new()));
        app.sql_hosts.insert(connection.id.clone(), host);
        app.collapsed
            .insert(crate::app::sql_tree::postgres::schema_key(
                &connection,
                namespace,
            ));
        let rows = db_tree::rows(
            &app.store,
            &app.tabs,
            &app.collapsed,
            db_tree::Hosts {
                remote: &app.remote_hosts,
                backups: &app.backups,
                queries: &app.queries,
                sql: &app.sql_hosts,
                ssh: &app.ssh_hosts,
                local: &app.local_trees,
                mongo: &app.mongo_hosts,
                redis: &app.redis_hosts,
            },
            "",
        );
        let row = rows
            .iter()
            .find(|row| matches!(&row.node,db_tree::Node::Schema(_,name,_) if *name==namespace))
            .unwrap();
        assert_eq!(row.icon(), expected);
        assert!(!rows.iter().any(|row| matches!(
            row.node,
            db_tree::Node::Table(..) | db_tree::Node::Query(..) | db_tree::Node::Backup(..)
        )));
        let filtered = db_tree::rows(
            &app.store,
            &app.tabs,
            &app.collapsed,
            db_tree::Hosts {
                remote: &app.remote_hosts,
                backups: &app.backups,
                queries: &app.queries,
                sql: &app.sql_hosts,
                ssh: &app.ssh_hosts,
                local: &app.local_trees,
                mongo: &app.mongo_hosts,
                redis: &app.redis_hosts,
            },
            "no-such-owned-object",
        );
        assert!(
            filtered.is_empty(),
            "search must not render object folders without a matching parent"
        );
    }
    Ok(())
}
