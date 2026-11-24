use anyhow::Result;
use tomato_core::queries::{Entry, Output, Request, Scope};
use tomato_services::queries::Repository;
#[test]
fn original_query_files_read_save_conflict_rename_nested_directory_and_delete_are_real()
-> Result<()> {
    let root = tempfile::tempdir()?;
    let repository = Repository::new(root.path().into());
    let scope = Scope {
        connection: "生产 A".into(),
        database: "my db".into(),
        path: String::new(),
    };
    let existing = root.path().join("生产_A/my_db/query/nested/中文.sql");
    std::fs::create_dir_all(existing.parent().unwrap())?;
    std::fs::write(&existing, "SELECT '旧查询🍅';\r\n")?;
    std::fs::write(existing.parent().unwrap().join("ignore.txt"), "not SQL")?;
    let Output::Listed { entries, .. } = repository.execute(Request::List(scope.clone()))? else {
        panic!("list")
    };
    assert_eq!(entries.len(), 1);
    assert!(entries[0].directory);
    let nested = scope.child("nested")?;
    let Output::Document { text, stamp, .. } = repository.execute(Request::Read {
        scope: nested.clone(),
        name: "中文".into(),
    })?
    else {
        panic!("read")
    };
    assert_eq!(text, "SELECT '旧查询🍅';\r\n");
    let Output::Saved {
        stamp: new_stamp, ..
    } = repository.execute(Request::Save {
        scope: nested.clone(),
        name: "中文".into(),
        text: "SELECT '修改';\n".into(),
        expected: Some(stamp.clone()),
    })?
    else {
        panic!("saved")
    };
    assert_ne!(stamp, new_stamp);
    assert_eq!(
        std::fs::read_to_string(existing.with_extension("sql.bak"))?,
        text
    );
    std::fs::write(&existing, "SELECT 'external';")?;
    assert!(
        repository
            .execute(Request::Save {
                scope: nested.clone(),
                name: "中文".into(),
                text: "must not overwrite".into(),
                expected: Some(new_stamp)
            })
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&existing)?, "SELECT 'external';");
    repository.execute(Request::Rename {
        scope: nested.clone(),
        entry: Entry {
            name: "中文".into(),
            directory: false,
        },
        name: "新查询".into(),
    })?;
    assert!(!existing.exists());
    assert!(existing.with_file_name("新查询.sql").exists());
    repository.execute(Request::Rename {
        scope: scope.clone(),
        entry: Entry {
            name: "nested".into(),
            directory: true,
        },
        name: "第二层".into(),
    })?;
    assert!(
        root.path()
            .join("生产_A/my_db/query/第二层/新查询.sql")
            .exists()
    );
    repository.execute(Request::Delete {
        scope: scope.clone(),
        entries: vec![Entry {
            name: "第二层".into(),
            directory: true,
        }],
    })?;
    assert!(!root.path().join("生产_A/my_db/query/第二层").exists());
    let Output::Listed { entries, .. } = repository.execute(Request::List(scope))? else {
        panic!("list")
    };
    assert!(entries.is_empty());
    Ok(())
}
#[test]
fn query_failure_does_not_truncate_existing_files_and_absent_lists_do_not_create_directories()
-> Result<()> {
    let root = tempfile::tempdir()?;
    let repository = Repository::new(root.path().into());
    let scope = Scope {
        connection: "local".into(),
        database: "main".into(),
        path: String::new(),
    };
    repository.execute(Request::List(scope.clone()))?;
    assert_eq!(std::fs::read_dir(root.path())?.count(), 0);
    repository.execute(Request::Save {
        scope: scope.clone(),
        name: "sample".into(),
        text: "original".into(),
        expected: None,
    })?;
    let path = root.path().join(scope.file("sample")?);
    assert!(
        repository
            .execute(Request::Save {
                scope: scope.clone(),
                name: "sample".into(),
                text: "x".repeat(8 * 1024 * 1024 + 1),
                expected: None
            })
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&path)?, "original");
    std::fs::create_dir(path.with_extension("sql.bak"))?;
    assert!(
        repository
            .execute(Request::Save {
                scope: scope.clone(),
                name: "sample".into(),
                text: "new".into(),
                expected: None
            })
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(&path)?, "original");
    let outside = Scope {
        path: "../outside".into(),
        ..scope
    };
    assert!(
        repository
            .execute(Request::Delete {
                scope: outside,
                entries: Vec::new()
            })
            .is_err()
    );
    Ok(())
}
