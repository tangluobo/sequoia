//! Sequential consumer deletion with the Java partial-failure contract.
use super::*;
use std::{collections::HashSet, future::Future, time::Duration};

pub(super) trait Remove {
    fn remove<'a>(&'a mut self, group: &'a str) -> impl Future<Output = Result<()>> + Send + 'a;
}

pub(super) async fn run(groups: Vec<String>, remover: &mut impl Remove) -> Result<Output> {
    ensure!(
        !groups.is_empty() && groups.len() <= 4096,
        "请选择 1–4096 个消费者组"
    );
    ensure!(
        groups.iter().all(|g| !g.is_empty() && g.len() <= 1024),
        "消费者组名称无效"
    );
    let mut seen = HashSet::new();
    let mut deleted = Vec::new();
    let mut failed = Vec::new();
    // Leave time for transport cleanup inside the command's 90-second bound.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    for group in groups {
        if !seen.insert(group.clone()) {
            continue;
        }
        let result = if tokio::time::Instant::now() >= deadline {
            Err(anyhow::anyhow!("本批操作已超时，此组未执行删除"))
        } else {
            match tokio::time::timeout_at(
                deadline.min(tokio::time::Instant::now() + Duration::from_secs(10)),
                remover.remove(&group),
            )
            .await
            {
                Ok(result) => result,
                Err(_) => Err(anyhow::anyhow!("删除响应超时，请刷新确认此组的状态")),
            }
        };
        match result {
            Ok(()) => deleted.push(group),
            Err(error) => {
                let mut error = format!("{error:#}");
                if error.len() > 1024 {
                    error.truncate(error.floor_char_boundary(1024));
                    error.push('…');
                }
                failed.push(json!({"group":group,"error":error}));
            }
        }
    }
    Ok(Output {
        rows: Vec::new(),
        message: if failed.is_empty() {
            format!("已删除 {} 个消费者组", deleted.len())
        } else {
            format!(
                "已删除 {} 个消费者组，{} 个删除失败",
                deleted.len(),
                failed.len()
            )
        },
        detail: json!({"deleted":deleted,"failed":failed}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn partial_failure_does_not_skip_later_groups_or_repeat_duplicates() -> Result<()> {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        struct Fake(std::sync::Arc<std::sync::Mutex<Vec<String>>>);
        impl Remove for Fake {
            async fn remove(&mut self, group: &str) -> Result<()> {
                self.0.lock().unwrap().push(group.to_owned());
                ensure!(group != "busy", "消费者组仍在使用中");
                Ok(())
            }
        }
        let mut fake = Fake(calls.clone());
        let report = run(
            ["first", "busy", "last", "first"]
                .map(str::to_owned)
                .to_vec(),
            &mut fake,
        )
        .await?;
        assert_eq!(*calls.lock().unwrap(), ["first", "busy", "last"]);
        assert_eq!(report.detail["deleted"], json!(["first", "last"]));
        assert_eq!(report.detail["failed"][0]["group"], "busy");
        assert!(report.message.contains("2 个消费者组"));
        assert!(run(Vec::new(), &mut fake).await.is_err());
        Ok(())
    }
}
