use super::*;
#[test]
fn directory_cache_bounds_memory_ignores_stale_replies_and_separates_actual_databases() {
    let mut cache = Cache::default();
    let scope = Scope {
        connection: "same-name".into(),
        database: "one".into(),
        path: String::new(),
    };
    let key = Key::new("id", "one", "");
    let first = cache.begin(key.clone(), scope.clone());
    let second = cache.begin(key.clone(), scope.clone());
    cache.finish(
        &key,
        first,
        Ok(vec![Item {
            name: "stale".into(),
            directory: false,
        }]),
    );
    assert!(cache.directories[&key].loading);
    cache.finish(
        &key,
        second,
        Ok(vec![Item {
            name: "current".into(),
            directory: false,
        }]),
    );
    assert_eq!(cache.directories[&key].items[0].name, "current");
    assert_ne!(key.tree_id(), Key::new("id", "two", "").tree_id());
    assert_ne!(
        Key::new("id:a", "b", "").tree_id(),
        Key::new("id", "a:b", "").tree_id()
    );
    for index in 0..100 {
        let path = index.to_string();
        let key = Key::new("id", "one", &path);
        let generation = cache.begin(
            key.clone(),
            Scope {
                path,
                ..scope.clone()
            },
        );
        cache.finish(
            &key,
            generation,
            Ok(vec![Item {
                name: "x".repeat(256 * 1024),
                directory: false,
            }]),
        );
    }
    assert!(
        cache.directories.len() <= 32,
        "8 MiB total metadata budget must evict before 64 entries"
    );
}
#[test]
fn backup_tree_file_actor_lists_renames_and_deletes_original_format_without_network()
-> anyhow::Result<()> {
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
    let mut connection = Connection::new("MYSQL");
    connection.name = "本地备份夹具".into();
    app.store.connections.push(connection.clone());
    let scope = Scope {
        connection: connection.name.clone(),
        database: "sample_db".into(),
        path: String::new(),
    };
    let folder = directory.path().join(scope.directory()?);
    std::fs::create_dir_all(&folder)?;
    std::fs::write(
        folder.join("source.nb3"),
        include_bytes!("../../../../services/tests/fixtures/backup/original.nb3"),
    )?;
    let sentinel = directory.path().join("connections.json");
    let before = std::fs::read(&sentinel).ok();
    let wait = |app: &mut TomatoApp| -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while std::time::Instant::now() < deadline {
            app.logic(&ctx);
            if !app
                .pending
                .values()
                .any(|pending| matches!(pending.purpose, Purpose::BackupTree(_)))
            {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        anyhow::bail!("backup file actor timed out")
    };
    app.list_backups(&connection.id, scope.clone())?;
    wait(&mut app)?;
    let key = Key::new(&connection.id, &scope.database, "");
    assert_eq!(app.backups.directories[&key].items[0].name, "source");
    app.backup_file_command(
        &connection.id,
        scope.clone(),
        FileAction::Rename {
            item: Item {
                name: "source".into(),
                directory: false,
            },
            name: "renamed".into(),
        },
        JobKind::Change,
    )?;
    wait(&mut app)?;
    assert!(!folder.join("source.nb3").exists());
    assert!(folder.join("renamed.nb3").exists());
    assert_eq!(app.backups.directories[&key].items[0].name, "renamed");
    app.backup_file_command(
        &connection.id,
        scope,
        FileAction::Delete {
            items: vec![Item {
                name: "renamed".into(),
                directory: false,
            }],
        },
        JobKind::Change,
    )?;
    wait(&mut app)?;
    assert!(!folder.join("renamed.nb3").exists());
    assert!(app.backups.directories[&key].items.is_empty());
    assert_eq!(std::fs::read(&sentinel).ok(), before);
    assert!(app.sql_hosts.is_empty());
    Ok(())
}

#[test]
fn postgres_backup_view_expansion_is_per_schema_but_legacy_disk_scope_is_shared() {
    let key = Key::new("host", "actual-database", "folder");
    assert_ne!(key.view_id(Some("public")), key.view_id(Some("other")));
    assert_eq!(key.view_id(None), key.tree_id());
    let mut c = Connection::new("POSTGRESQL");
    c.name = "legacy".into();
    let a = target(&c, "actual-database", "folder", None, Some("public"));
    let b = target(&c, "actual-database", "folder", None, Some("other"));
    assert_eq!(
        a.directory().unwrap().directory().unwrap(),
        b.directory().unwrap().directory().unwrap()
    );
    assert_eq!(a.namespace.as_deref(), Some("public"));
}
