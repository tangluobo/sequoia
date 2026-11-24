//! Full GUI-command-database-file roundtrip, restricted to the owned QA runner.
use super::*;
use crate::app::tests::native_bridge::Bridge;
use anyhow::Context;
use std::time::{Duration, Instant};
fn click_object_toolbar(app: &mut TomatoApp, bridge: &Bridge, label: &str) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    let output = bridge.root_frame(&ctx, vec![], vec![], |ui| app.ui(ui));
    let point = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == label => {
            Some(text.pos + text.galley.size() / 2.)
        }
        _ => None,
    });
    let pos = point.with_context(|| format!("missing object toolbar control {label}"))?;
    for pressed in [true, false] {
        bridge.root_frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![],
            |ui| app.ui(ui),
        );
    }
    Ok(())
}
#[track_caller]
fn wait(app: &mut TomatoApp, done: impl Fn(&TomatoApp) -> bool) -> anyhow::Result<()> {
    let ctx = app.context.clone();
    let oracle = app.store.connections.iter().any(|c| c.kind() == "ORACLE");
    let caller = std::panic::Location::caller();
    let started = Instant::now();
    let deadline = started + Duration::from_secs(if oracle { 480 } else { 30 });
    if oracle {
        eprintln!("Oracle backup UI waiting at {caller}");
    }
    while Instant::now() < deadline {
        app.logic(&ctx);
        if let Some(error) = app
            .backup_dialog
            .as_ref()
            .and_then(|draft| draft.error.as_deref())
        {
            anyhow::bail!("native backup actor failed: {error}");
        }
        if done(app) {
            if oracle {
                eprintln!(
                    "Oracle backup UI complete at {caller} after {:?}",
                    started.elapsed()
                );
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let state = app.backup_dialog.as_ref().map(|draft| {
        (
            draft.loading,
            draft.running,
            draft.restore.is_some(),
            draft.complete.is_some(),
        )
    });
    anyhow::bail!(
        "native backup actor timed out at {caller}; pending={}, draft (loading, running, restore, complete)={state:?}",
        app.pending.len(),
    )
}
#[test]
#[ignore = "requires owned loopback MySQL fixture and isolated native UI harness"]
fn mysql_native_backup_and_restore_controls_roundtrip_real_data_and_preserve_configuration()
-> anyhow::Result<()> {
    run("MYSQL", true)
}
#[test]
#[ignore = "requires owned loopback PostgreSQL fixture and isolated native UI harness"]
fn postgres_native_backup_and_restore_controls_roundtrip_real_data_and_preserve_configuration()
-> anyhow::Result<()> {
    run("POSTGRESQL", true)
}
#[test]
#[ignore = "requires owned loopback Oracle fixture and isolated native UI harness"]
fn oracle_native_backup_and_restore_controls_roundtrip_real_data_and_preserve_configuration()
-> anyhow::Result<()> {
    run("ORACLE", false)
}
#[test]
#[ignore = "requires owned loopback Oracle fixture and isolated native UI harness"]
fn oracle_native_backup_object_toolbar_roundtrips_real_data_and_preserves_configuration()
-> anyhow::Result<()> {
    run("ORACLE", true)
}
fn run(kind: &str, toolbar: bool) -> anyhow::Result<()> {
    let pg = kind == "POSTGRESQL";
    let oracle = kind == "ORACLE";
    let (port, database) = if oracle {
        let database = match std::env::var("TOMATO_TEST_ORACLE_ISOLATED").as_deref() {
            Ok("owned-container-freepdb1") => "/FREEPDB1",
            Ok("owned-container-xepdb1") => "/XEPDB1",
            _ => anyhow::bail!("owned loopback Oracle fixture required"),
        };
        (
            std::env::var("TOMATO_TEST_ORACLE_PORT")?.parse::<u16>()?,
            database.into(),
        )
    } else {
        let url = std::env::var(if pg {
            "TOMATO_TEST_POSTGRES_URL"
        } else {
            "TOMATO_TEST_MYSQL_URL"
        })?;
        let prefix = if pg {
            "postgres://tomato_fixture@127.0.0.1:"
        } else {
            "mysql://root@127.0.0.1:"
        };
        let endpoint = url
            .strip_prefix(prefix)
            .ok_or_else(|| anyhow::anyhow!("only owned loopback fixture URLs are accepted"))?;
        let (port, database) = endpoint
            .split_once('/')
            .ok_or_else(|| anyhow::anyhow!("fixture URL has no database"))?;
        let port = port.parse::<u16>()?;
        anyhow::ensure!(
            port > 0
                && database.starts_with("tomato_fixture")
                && database
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
            "unsafe fixture endpoint"
        );
        (port, database.to_owned())
    };
    anyhow::ensure!(port > 0, "fixture port missing");
    let directory = tempfile::tempdir()?;
    let config = directory.path().join("connections.json");
    let repository = ConnectionFileRepository::new(config.clone());
    repository.save(&repository.load()?)?;
    let original = std::fs::read(&config)?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(&ctx, runtime, repository);
    let bridge = Bridge::new(&ctx);
    let mut base = Connection::new(kind);
    base.name = "native backup UI fixture".into();
    base.set("host", "127.0.0.1");
    base.set("port", port);
    base.set(
        "username",
        if oracle {
            "system"
        } else if pg {
            "tomato_fixture"
        } else {
            "root"
        },
    );
    base.set("database", database.as_str());
    if oracle {
        base.set("password", std::env::var("TOMATO_TEST_ORACLE_PASSWORD")?);
    }
    let mut owned = format!(
        "tomato_backup_ui_{}",
        tomato_core::Connection::new("TOOL").id.replace('-', "")
    );
    if oracle {
        owned.truncate(30);
        owned.make_ascii_uppercase();
    }
    anyhow::ensure!(
        owned
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_'),
        "invalid owned database name"
    );
    let qualified = tomato_services::dialect::resolve(kind)?.quote(&owned)?;
    let mut connection = tomato_core::database_scope::select(&base, &owned);
    if oracle {
        connection.id = Connection::new(kind).id;
        connection.set("username", owned.as_str());
        connection.set("password", "OwnedFixture42");
    }
    app.store.connections.push(connection.clone());
    let scope = tomato_core::backup::Scope {
        connection: connection.name.clone(),
        database: owned.clone(),
        path: String::new(),
    };
    let mut admin = tomato_services::Databases::default();
    let mut created = false;
    let outcome = (|| -> anyhow::Result<()> {
        app.runtime.as_ref().unwrap().block_on(admin.query(
            &base,
            &if oracle {
                format!("CREATE USER {qualified} IDENTIFIED BY \"OwnedFixture42\" DEFAULT TABLESPACE USERS QUOTA UNLIMITED ON USERS")
            } else {format!(
                "CREATE DATABASE {} {}",
                qualified,
                if pg { "" } else { "CHARACTER SET utf8mb4" }
            )},
        ))?;
        created = true;
        if oracle {
            app.runtime.as_ref().unwrap().block_on(admin.query(
                &base,
                &format!(
                    "GRANT CREATE SESSION, CREATE TABLE, CREATE VIEW, CREATE TRIGGER TO {qualified}"
                ),
            ))?;
        }
        app.runtime.as_ref().unwrap().block_on(admin.query(&connection, if oracle {
            r#"CREATE TABLE "items"(id NUMBER PRIMARY KEY,label VARCHAR2(100));INSERT INTO "items" VALUES(1,'中文');INSERT INTO "items" VALUES(2,'original');CREATE VIEW item_view AS SELECT id,label FROM "items""#
        } else {
            "CREATE TABLE items(id INT PRIMARY KEY,label TEXT);INSERT INTO items VALUES(1,'中文'),(2,'original');CREATE VIEW item_view AS SELECT id,label FROM items"
        }))?;
        let table_sql = if oracle { "\"items\"" } else { "items" };
        let select = format!("SELECT id,label FROM {table_sql} ORDER BY id");
        let expected = app
            .runtime
            .as_ref()
            .unwrap()
            .block_on(admin.query(&connection, &select))?
            .remove(0)
            .rows;
        let schema = if pg { "public" } else { owned.as_str() };
        app.open_table(
            connection.clone(),
            tomato_core::workbench::TableInfo {
                table: tomato_core::workbench::TableId {
                    schema: schema.into(),
                    name: "items".into(),
                },
                view: false,
            },
            false,
        );
        wait(&mut app, |app| app.pending.is_empty())?;
        let data_id = app.active.clone().context("table tab")?;
        if oracle {
            use crate::app::sql_tree::postgres::tests::menu;
            app.open_direct_host(connection.clone(), false);
            wait(&mut app, |app| app.pending.is_empty())?;
            app.filter = owned.clone();
            menu(&mut app, &owned, 0, "打开").context("open Oracle owner in tree")?;
            wait(&mut app, |app| app.pending.is_empty())?;
            if !toolbar {
                menu(&mut app, "备份", 0, "新建备份").context("open backup from tree")?;
            }
        }
        if toolbar {
            app.open_objects(connection.clone(), schema.into());
            wait(&mut app, |app| app.pending.is_empty())?;
            click_object_toolbar(&mut app, &bridge, "导出向导")?;
            anyhow::ensure!(
                app.data_export.is_none(),
                "object toolbar must open a backup"
            );
        }
        wait(&mut app, |app| {
            app.backup_dialog
                .as_ref()
                .is_some_and(|draft| !draft.loading)
        })?;
        anyhow::ensure!(
            app.backup_dialog.as_ref().unwrap().namespace.as_deref()
                == if pg { Some("public") } else { None },
            "tree schema reaches backup actor"
        );
        bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
        bridge.click(&ctx, egui::pos2(100., 180.), |ctx| app.backup_dialog(ctx));
        bridge.frame(
            &ctx,
            vec![egui::Event::Paste("通过控件生成".into())],
            |ctx| app.backup_dialog(ctx),
        );
        bridge.click(&ctx, egui::pos2(146., 18.), |ctx| app.backup_dialog(ctx));
        bridge.click(&ctx, egui::pos2(7., 78.), |ctx| app.backup_dialog(ctx));
        bridge.click(&ctx, egui::pos2(7., 111.), |ctx| app.backup_dialog(ctx));
        bridge.click(&ctx, egui::pos2(100., 149.), |ctx| app.backup_dialog(ctx));
        bridge.frame(
            &ctx,
            vec![egui::Event::Paste(" ui_native ".into())],
            |ctx| app.backup_dialog(ctx),
        );
        let draft = app.backup_dialog.as_ref().unwrap();
        assert_eq!(draft.options.comment, "通过控件生成");
        assert!(draft.options.single_transaction);
        assert!(draft.options.custom_filename);
        assert_eq!(draft.options.filename, " ui_native ");
        bridge.click(&ctx, egui::pos2(85., 18.), |ctx| app.backup_dialog(ctx));
        bridge.click(&ctx, egui::pos2(300., 516.), |ctx| app.backup_dialog(ctx));
        assert_eq!(app.backup_dialog.as_ref().unwrap().objects().len(), 2);
        bridge.click(&ctx, egui::pos2(575., 556.), |ctx| app.backup_dialog(ctx));
        wait(&mut app, |app| {
            app.backup_dialog
                .as_ref()
                .is_some_and(|draft| draft.complete.is_some())
                && app.pending.is_empty()
        })?;
        let path = directory.path().join(scope.file(" ui_native ")?);
        assert!(path.is_file());
        let mut archive = tomato_services::backup::Archive::open(&path)?;
        let metadata: tomato_core::backup::Metadata = archive.json("meta.json", false)?;
        assert_eq!(metadata.comment, "通过控件生成");
        assert_eq!(metadata.objects.len(), 2);
        assert_eq!(metadata.database_type, kind);
        if oracle {
            assert_eq!(metadata.schema, owned);
        }
        if let Some(draft) = app.backup_dialog.take() {
            draft.release(&ctx);
        }
        app.runtime.as_ref().unwrap().block_on(admin.query(
            &connection,
            &format!("DELETE FROM {table_sql} WHERE id=1;UPDATE {table_sql} SET label='changed'"),
        ))?;
        let key = crate::app::backup_tree::Key::new(&connection.id, &owned, "");
        let archive_item = app.backups.directories[&key]
            .items
            .iter()
            .find(|item| item.name == " ui_native " && !item.directory)
            .context("new archive was not listed in the backup tree")?
            .clone();
        if !toolbar {
            use crate::app::sql_tree::postgres::tests::menu;
            // Expanding the cached folder leaves its archive visible through restore.
            app.backup_action(
                crate::app::backup_tree::target(&connection, &owned, "", None, None),
                "expand",
            );
            wait(&mut app, |app| app.pending.is_empty())?;
            menu(&mut app, &archive_item.name, 0, "还原备份")
                .context("restore the newly listed archive")?;
        } else {
            let source = if oracle {
                scope.clone()
            } else {
                let nested = scope.child("历史备份")?;
                let nested_path = directory.path().join(nested.file(&archive_item.name)?);
                std::fs::create_dir_all(nested_path.parent().unwrap())?;
                std::fs::rename(&path, &nested_path)?;
                nested
            };
            click_object_toolbar(&mut app, &bridge, "导入向导")?;
            wait(&mut app, |app| app.pending.is_empty())?;
            anyhow::ensure!(
                app.data_import.is_none(),
                "object toolbar must open restore"
            );
            bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
            bridge.click(&ctx, egui::pos2(470., 456.), |ctx| app.backup_dialog(ctx));
            assert!(!app.backup_dialog.as_ref().unwrap().confirm_restore);
            let picker = bridge.text("选择备份文件")?;
            bridge.click(&ctx, picker, |ctx| app.backup_dialog(ctx));
            bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
            if !source.path.is_empty() {
                let folder = bridge.text("历史备份/")?;
                bridge.click(&ctx, folder, |ctx| app.backup_dialog(ctx));
                wait(&mut app, |app| app.pending.is_empty())?;
                bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
            }
            let file = bridge.text(&format!("{}.nb3", archive_item.name))?;
            bridge.click(&ctx, file, |ctx| app.backup_dialog(ctx));
            let draft = app.backup_dialog.as_ref().unwrap();
            assert_eq!(draft.scope.path, source.path);
            assert_eq!(draft.restore.as_deref(), Some(archive_item.name.as_str()));
        }
        let backup_tree_was_collapsed = app.collapsed.contains(&key.tree_id());
        bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
        bridge.click(&ctx, egui::pos2(470., 456.), |ctx| app.backup_dialog(ctx));
        bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
        assert!(app.backup_dialog.as_ref().unwrap().confirm_restore);
        assert!(app.pending.is_empty());
        let no = bridge.text("否")?;
        bridge.click(&ctx, no, |ctx| app.backup_dialog(ctx));
        assert!(!app.backup_dialog.as_ref().unwrap().confirm_restore);
        assert!(app.pending.is_empty());
        bridge.click(&ctx, egui::pos2(470., 456.), |ctx| app.backup_dialog(ctx));
        bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
        let yes = bridge.text("是")?;
        bridge.click(&ctx, yes, |ctx| app.backup_dialog(ctx));
        wait(&mut app, |app| {
            app.backup_dialog
                .as_ref()
                .is_some_and(|draft| draft.complete.is_some())
                && app.pending.is_empty()
        })?;
        let actual = app
            .runtime
            .as_ref()
            .unwrap()
            .block_on(admin.query(&connection, &select))?
            .remove(0)
            .rows;
        assert_eq!(actual, expected);
        let tab = app
            .tabs
            .iter()
            .find(|tab| tab.id == data_id)
            .context("data tab remains open")?;
        anyhow::ensure!(
            tab.error.is_none(),
            "restore refresh failed: {:?}",
            tab.error
        );
        let Panel::Sql { workbench, .. } = &tab.panel else {
            anyhow::bail!("SQL data tab")
        };
        let page = workbench.page.as_ref().context("refreshed table page")?;
        anyhow::ensure!(
            page.rows.len() == 2
                && page.rows[0][1] == tomato_core::workbench::Cell::Text("中文".into()),
            "actual data tab refreshed after restore"
        );
        if oracle {
            assert_eq!(
                app.collapsed.contains(&key.tree_id()),
                backup_tree_was_collapsed
            );
            assert!(
                app.backups.directories[&key]
                    .items
                    .iter()
                    .any(|item| item.name == archive_item.name)
            );
            let rows = crate::app::sql_tree::postgres::tests::frame(&mut app, vec![]);
            assert_eq!(
                rows.iter().any(|(label, _)| label == &archive_item.name),
                !backup_tree_was_collapsed,
            );
        }
        assert_eq!(std::fs::read(&config)?, original);
        assert!(app.error.is_none());
        Ok(())
    })();
    if let Some(draft) = app.backup_dialog.take() {
        draft.release(&ctx);
    }
    let disconnected = (|| -> anyhow::Result<()> {
        let id = app.queue.submit(Command::Disconnect {
            id: connection.id.clone(),
        })?;
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            for result in app.queue.poll() {
                if result.id == id {
                    result.result?;
                    return Ok(());
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        anyhow::bail!("owned native session did not disconnect")
    })();
    let cleanup = app.runtime.as_ref().unwrap().block_on(async {
        admin.disconnect(&connection.id).await?;
        admin.disconnect(&base.id).await?;
        if !created {
            return Ok(());
        }
        let drop = if oracle {
            format!("DROP USER {qualified} CASCADE")
        } else {
            format!("DROP DATABASE IF EXISTS {qualified}")
        };
        let result = admin.query(&base, &drop).await;
        let disconnected = admin.disconnect(&base.id).await;
        result.map(|_| ()).and(disconnected)
    });
    outcome.and(disconnected).and(cleanup)
}
