use super::*;
use sqlx::{Connection as _, SqliteConnection, sqlite::SqliteConnectOptions};

pub(super) async fn tasks(config: Config, phone: String, generation: u64) -> Result<Output> {
    validate(&config)?;
    let dir = directory(&config, &phone)?;
    if !dir.is_dir() {
        return Ok(Output::Tasks {
            phone,
            generation,
            tasks: Vec::new(),
            status: "会话目录不存在".into(),
            truncated: false,
        });
    }
    let workspace = dir.join("workspaceStorage");
    if !workspace.is_dir() {
        return Ok(Output::Tasks {
            phone,
            generation,
            tasks: Vec::new(),
            status: "未找到 workspaceStorage 目录".into(),
            truncated: false,
        });
    }
    let storage = read(&dir.join("globalStorage/storage.json"))?;
    let mut keys: Vec<String> = storage
        .map(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes))
        .transpose()?
        .and_then(|json| {
            json.pointer("/windowSplashWorkspaceOverride/layoutInfo/workspaces")
                .and_then(serde_json::Value::as_object)
                .map(|v| v.keys().cloned().collect())
        })
        .unwrap_or_default();
    if keys.is_empty() {
        for entry in fs::read_dir(&workspace)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                keys.push(entry.file_name().to_string_lossy().into_owned());
            }
            ensure!(keys.len() <= 10000, "工作区超过 10000 项");
        }
        keys.sort();
    }
    let canonical = fs::canonicalize(&workspace)?;
    let mut tasks = Vec::new();
    let mut retained = 0usize;
    let mut truncated = false;
    let mut errors = 0;
    for key in keys.into_iter().take(10000) {
        component(&key)?;
        let path = workspace.join(key).join("state.vscdb");
        if !path.is_file() {
            continue;
        }
        ensure!(
            fs::canonicalize(&path)?.starts_with(&canonical),
            "工作区数据库链接指向会话目录之外"
        );
        let options = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .create_if_missing(false)
            .statement_cache_capacity(0)
            .busy_timeout(std::time::Duration::from_secs(2));
        let mut connection = match SqliteConnection::connect_with(&options).await {
            Ok(c) => c,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        let result = read_history(&mut connection).await;
        let _ = connection.close().await;
        let values = match result {
            Ok(v) => v,
            Err(_) => {
                errors += 1;
                continue;
            }
        };
        for task in values.into_iter().rev() {
            if tasks.len() >= 10000 || retained.saturating_add(task.len()) > LIMIT {
                truncated = true;
                break;
            }
            retained += task.len();
            tasks.push(task);
        }
        if truncated {
            break;
        }
    }
    let status = if errors > 0 {
        format!("部分工作区历史无法读取（{errors} 个）")
    } else if tasks.is_empty() {
        "暂无历史任务".into()
    } else {
        String::new()
    };
    Ok(Output::Tasks {
        phone,
        generation,
        tasks,
        status,
        truncated,
    })
}
async fn value(connection: &mut SqliteConnection, key: &str) -> Result<Option<String>> {
    // Bound bytes before SQLx allocates the SQLite column, including BLOB-backed VS Code stores.
    let value:Option<(i64,String)>=sqlx::query_as("SELECT length(CAST(value AS BLOB)), CAST(substr(CAST(value AS BLOB),1,?) AS TEXT) FROM ItemTable WHERE key=? LIMIT 1")
        .bind((LIMIT+1) as i64).bind(key).fetch_optional(connection).await?;
    if let Some((length, text)) = value {
        ensure!(length <= LIMIT as i64, "工作区历史超过 8 MiB");
        Ok(Some(text))
    } else {
        Ok(None)
    }
}
async fn read_history(connection: &mut SqliteConnection) -> Result<Vec<String>> {
    let mut tasks = Vec::new();
    if let Some(text) = value(connection, "icube-ai-agent-storage-input-history").await?
        && !text.trim().is_empty()
    {
        let json: serde_json::Value = serde_json::from_str(&text)?;
        if let Some(array) = json.as_array() {
            for item in array {
                if let Some(text) = item.get("inputText").and_then(serde_json::Value::as_str) {
                    tasks.push(text.into());
                }
            }
        }
    }
    if tasks.is_empty()
        && let Some(text) = value(connection, "memento/icube-ai-agent-storage").await?
        && !text.trim().is_empty()
    {
        let json: serde_json::Value = serde_json::from_str(&text)?;
        if let Some(array) = json.get("list").and_then(serde_json::Value::as_array) {
            for item in array {
                if let Some(id) = item
                    .get("sessionId")
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    tasks.push(format!(
                        "会话: {id}{}",
                        if item.get("isCurrent").and_then(serde_json::Value::as_bool) == Some(true)
                        {
                            " (当前)"
                        } else {
                            ""
                        }
                    ));
                }
            }
        }
    }
    Ok(tasks)
}
