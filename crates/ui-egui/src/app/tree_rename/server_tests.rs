//! Full sidebar rename gestures against separately owned MySQL/PostgreSQL instances.
use super::tests::Driver;
use super::*;
use crate::app::sql_tree::postgres::tests::{menu, wait};
use anyhow::Context;
fn fixture(kind: &str) -> anyhow::Result<()> {
    let (var, prefix, user) = if kind == "MYSQL" {
        ("TOMATO_TEST_MYSQL_URL", "mysql://root@127.0.0.1:", "root")
    } else {
        (
            "TOMATO_TEST_POSTGRES_URL",
            "postgres://tomato_fixture@127.0.0.1:",
            "tomato_fixture",
        )
    };
    let url = std::env::var(var)?;
    let endpoint = url.strip_prefix(prefix).context("owned loopback only")?;
    let (port, database) = endpoint.split_once('/').context("database")?;
    anyhow::ensure!(database == "tomato_fixture");
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("connections.json");
    let repo = ConnectionFileRepository::new(&path);
    repo.save(&repo.load()?)?;
    let before = std::fs::read(&path)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repo);
    let mut connection = Connection::new(kind);
    connection.name = "owned-sidebar-rename".into();
    connection.set("host", "127.0.0.1");
    connection.set("port", u64::from(port.parse::<u16>()?));
    connection.set("username", user);
    connection.set("password", "");
    connection.set("database", database);
    app.store.connections.push(connection.clone());
    let schema = format!(
        "tomato_fixture_rename_{}",
        Connection::new("TOOL").id.replace('-', "")
    );
    let namespace = if kind == "MYSQL" {
        "DATABASE"
    } else {
        "SCHEMA"
    };
    let dialect = tomato_services::dialect::resolve(kind)?;
    let qschema = dialect.quote(&schema)?;
    let q = |name: &str| -> anyhow::Result<String> {
        Ok(format!("{qschema}.{}", dialect.quote(name)?))
    };
    let mut backend = tomato_services::Databases::default();
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(backend.query(&connection, &format!("CREATE {namespace} {qschema}")))?;
    let mut routed = connection.clone();
    if kind == "MYSQL" {
        routed.set("database", schema.clone());
    }
    app.runtime.as_ref().unwrap().block_on(async {
        for name in ["rename_table","occupied_table"]{backend.query(&routed,&format!("CREATE TABLE {}(id INTEGER PRIMARY KEY,label VARCHAR(50));INSERT INTO {} VALUES(1,'kept')",q(name)?,q(name)?)).await?;}
        backend.query(&routed,&format!("CREATE VIEW {} AS SELECT * FROM {}",q("rename_view")?,q("occupied_table")?)).await?;Ok::<_,anyhow::Error>(())
    })?;
    let mut d = Driver::wrap(app, directory, connection.clone());
    let outcome = (|| -> anyhow::Result<()> {
        d.app.open_connection(connection.clone());
        wait(&mut d.app)?;
        if kind == "POSTGRESQL" {
            menu(&mut d.app, database, 0, "打开")?;
            wait(&mut d.app)?;
        }
        menu(&mut d.app, &schema, 0, "打开")?;
        wait(&mut d.app)?;
        for folder in ["表", "视图"] {
            let p = d
                .frame(vec![])
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == folder => {
                        Some(egui::pos2(t.pos.x - 32., t.pos.y + t.galley.size().y / 2.))
                    }
                    _ => None,
                })
                .context("folder disclosure")?;
            d.click(p, egui::PointerButton::Primary, egui::Modifiers::NONE);
            d.drain()?;
        }
        d.point("rename_table");
        d.point("rename_view");
        let info = TableInfo {
            table: tomato_core::workbench::TableId {
                schema: schema.clone(),
                name: "rename_table".into(),
            },
            view: false,
        };
        d.app.open_table(routed.clone(), info, false);
        d.drain()?;
        let tab = d.app.active.clone().context("open table tab")?;
        d.start("rename_table");
        d.type_name("renamed_table");
        d.key(egui::Key::Enter);
        d.drain()?;
        d.point("renamed_table");
        anyhow::ensure!(d.app.tree_objects.is_none());
        anyhow::ensure!(
            d.app.tabs.iter().any(|t| t.id == tab
                && matches!(&t.panel,Panel::Sql{workbench,..} if workbench.needs_refresh)),
            "old table tab must be marked stale"
        );
        d.select("rename_view");
        d.key(egui::Key::F2);
        d.frame(vec![]);
        d.assert_editor();
        d.type_name("renamed_view");
        d.key(egui::Key::Enter);
        d.drain()?;
        d.point("renamed_view");
        for name in ["renamed_table", "renamed_view"] {
            let count =
                d.app.runtime.as_ref().unwrap().block_on(
                    backend.query(&routed, &format!("SELECT count(*) FROM {}", q(name)?)),
                )?;
            anyhow::ensure!(count[0].rows[0][0] == "1");
        }
        d.start("renamed_table");
        d.type_name("occupied_table");
        d.key(egui::Key::Enter);
        d.drain()?;
        d.point("renamed_table");
        anyhow::ensure!(
            d.app.tree_objects.is_none() && d.app.status.contains("重命名失败"),
            "collision must retain old row and expose failure without a rename modal"
        );
        anyhow::ensure!(std::fs::read(&path)? == before);
        Ok(())
    })();
    d.app.disconnect_sql(&connection.id);
    wait(&mut d.app)?;
    d.app.runtime.as_ref().unwrap().block_on(async {
        backend.disconnect(&connection.id).await?;
        backend
            .query(
                &connection,
                &format!(
                    "DROP {namespace} {qschema}{}",
                    if kind == "POSTGRESQL" { " CASCADE" } else { "" }
                ),
            )
            .await?;
        backend.disconnect(&connection.id).await
    })?;
    outcome
}
#[test]
#[ignore = "owned MySQL loopback fixture required"]
fn mysql_sidebar_inline_rename_preserves_data_and_refreshes_related_tabs() -> anyhow::Result<()> {
    fixture("MYSQL")
}
#[test]
#[ignore = "owned PostgreSQL loopback fixture required"]
fn postgres_sidebar_inline_rename_preserves_data_and_refreshes_related_tabs() -> anyhow::Result<()>
{
    fixture("POSTGRESQL")
}
