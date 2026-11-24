use anyhow::{Context, Result};
use std::{fs, path::Path};
use tomato_services::trae::{self, Action, Config, Mutation, Output, Session, Snapshot};
fn config(path: &Path, user: &Path) -> Result<()> {
    let json = serde_json::json!({"traeUserDir":user,"sessions":[{"phone":"111","name":"工作账号","active":false,"sessionFuture":7},{"phone":"222","name":"测试账号","active":false}],"future":{"retained":true}});
    fs::write(path, serde_json::to_vec(&json)?)?;
    Ok(())
}
async fn mutate(path: &Path, previous: &Snapshot, mutation: Mutation) -> Result<Snapshot> {
    match trae::execute(
        path.into(),
        Action::Mutate {
            previous: Box::new(previous.clone()),
            mutation,
        },
    )
    .await?
    {
        Output::Snapshot(snapshot) => Ok(snapshot),
        _ => anyhow::bail!("expected snapshot"),
    }
}
#[tokio::test]
async fn legacy_sessions_archive_switch_close_delete_reorder_and_conflicts() -> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("trae_sessions.json");
    let user = root.path().join("User");
    config(&path, &user)?;
    for (name, value) in [
        ("User", "unregistered"),
        ("User111", "work"),
        ("User222", "test"),
    ] {
        let dir = root.path().join(name);
        fs::create_dir(&dir)?;
        fs::write(dir.join("identity"), value)?;
    }
    let bytes = fs::read(&path)?;
    let snapshot = trae::load(&path)?;
    assert_eq!(fs::read(&path)?, bytes);
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Activate {
            phone: "111".into(),
        },
    )
    .await?;
    assert_eq!(fs::read_to_string(user.join("identity"))?, "work");
    assert_eq!(
        fs::read_to_string(root.path().join("User_unknown_archive_1/identity"))?,
        "unregistered"
    );
    assert!(snapshot.config.sessions[0].active);
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Activate {
            phone: "222".into(),
        },
    )
    .await?;
    assert_eq!(
        fs::read_to_string(root.path().join("User111/identity"))?,
        "work"
    );
    assert_eq!(fs::read_to_string(user.join("identity"))?, "test");
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Close {
            phone: "222".into(),
        },
    )
    .await?;
    assert!(!user.exists());
    fs::create_dir(&user)?;
    fs::write(user.join("identity"), "third")?;
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Archive {
            phone: "333".into(),
            name: "第三账号".into(),
        },
    )
    .await?;
    assert_eq!(
        fs::read_to_string(root.path().join("User333/identity"))?,
        "third"
    );
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Reorder {
            phones: vec!["333".into(), "222".into(), "111".into()],
        },
    )
    .await?;
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Rename {
            phone: "111".into(),
            name: "已改名".into(),
        },
    )
    .await?;
    assert_eq!(snapshot.config.sessions[2].extra["sessionFuture"], 7);
    assert_eq!(snapshot.config.extra["future"]["retained"], true);
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Delete {
            phone: "333".into(),
            delete_data: false,
        },
    )
    .await?;
    assert!(root.path().join("User333").is_dir());
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Delete {
            phone: "222".into(),
            delete_data: true,
        },
    )
    .await?;
    assert!(!root.path().join("User222").exists());
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Activate {
            phone: "111".into(),
        },
    )
    .await?;
    let snapshot = mutate(
        &path,
        &snapshot,
        Mutation::Delete {
            phone: "111".into(),
            delete_data: true,
        },
    )
    .await?;
    assert!(snapshot.config.sessions.is_empty());
    assert_eq!(fs::read_to_string(user.join("identity"))?, "work");
    fs::write(&path, bytes)?;
    assert!(
        mutate(
            &path,
            &snapshot,
            Mutation::Archive {
                phone: "444".into(),
                name: "conflict".into()
            }
        )
        .await
        .is_err()
    );
    assert!(user.is_dir());
    assert!(path.with_extension("json.bak").is_file());
    Ok(())
}
#[cfg(windows)]
#[tokio::test]
async fn failed_config_commit_rolls_back_directory_moves() -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir()?;
    let path = root.path().join("trae_sessions.json");
    let user = root.path().join("User");
    config(&path, &user)?;
    fs::create_dir(&user)?;
    fs::write(user.join("identity"), "keep")?;
    let snapshot = trae::load(&path)?;
    // Readers are allowed, replacement is denied. Failure happens after the actual rename.
    let lock = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)?;
    let result = mutate(
        &path,
        &snapshot,
        Mutation::Archive {
            phone: "333".into(),
            name: "rollback".into(),
        },
    )
    .await;
    assert!(result.is_err());
    assert_eq!(fs::read_to_string(user.join("identity"))?, "keep");
    assert!(!root.path().join("User333").exists());
    drop(lock);
    assert_eq!(trae::load(&path)?.stamp, snapshot.stamp);
    Ok(())
}
#[tokio::test]
async fn sqlite_history_uses_workspace_keys_fallback_and_read_only_connections() -> Result<()> {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let root = tempfile::tempdir()?;
    let user = root.path().join("User");
    let workspace = user.join("workspaceStorage/one");
    fs::create_dir_all(&workspace)?;
    let db = workspace.join("state.vscdb");
    let options = SqliteConnectOptions::new()
        .filename(&db)
        .create_if_missing(true);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query("CREATE TABLE ItemTable (key TEXT PRIMARY KEY,value BLOB)")
        .execute(&mut connection)
        .await?;
    sqlx::query("INSERT INTO ItemTable VALUES (?,?)")
        .bind("icube-ai-agent-storage-input-history")
        .bind(br#"[{"inputText":"old"},{"inputText":"new"}]"#.as_slice())
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let before = fs::read(&db)?;
    let config = Config {
        user_dir: user.clone(),
        sessions: vec![Session {
            phone: "111".into(),
            active: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    let request = || Action::Tasks {
        config: config.clone(),
        phone: "111".into(),
        generation: 9,
    };
    let result = trae::execute(root.path().join("unused.json"), request()).await?;
    assert!(matches!(result,Output::Tasks {generation:9,tasks,..} if tasks==["new","old"]));
    assert_eq!(fs::read(&db)?, before);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    sqlx::query("DELETE FROM ItemTable")
        .execute(&mut connection)
        .await?;
    sqlx::query("INSERT INTO ItemTable VALUES (?,?)")
        .bind("memento/icube-ai-agent-storage")
        .bind(r#"{"list":[{"sessionId":"s1","isCurrent":true},{"sessionId":"s2"}]}"#)
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    fs::create_dir_all(user.join("globalStorage"))?;
    fs::write(
        user.join("globalStorage/storage.json"),
        r#"{"windowSplashWorkspaceOverride":{"layoutInfo":{"workspaces":{"one":{}}}}}"#,
    )?;
    let result = trae::execute(root.path().join("unused.json"), request()).await?;
    assert!(matches!(result,Output::Tasks {tasks,..} if tasks==["会话: s2","会话: s1 (当前)"]));
    // No lingering connection may lock the workspace after completion.
    fs::rename(&db, workspace.join("moved.vscdb")).context("history left database locked")?;
    Ok(())
}
