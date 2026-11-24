//! Preserve per-key outcomes, including work completed before a transport failure.
use super::*;

pub(super) async fn delete_keys(stream: &mut Session, keys: Vec<String>) -> Result<Output> {
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(25);
    let mut succeeded = 0;
    let mut failed = 0;
    let mut failures = Vec::new();
    let mut retained = 0;
    let mut stopped: Option<&str> = None;
    for key in keys {
        let failure = if let Some(reason) = stopped {
            Some(reason.to_owned())
        } else {
            match tokio::time::timeout_at(
                deadline,
                query::<i64>(stream, redis::cmd("DEL").arg(&key)),
            )
            .await
            {
                Ok(Ok(_)) => {
                    succeeded += 1;
                    None
                }
                Ok(Err(error)) => {
                    if error
                        .downcast_ref::<redis::RedisError>()
                        .is_some_and(redis::RedisError::is_io_error)
                    {
                        stopped = Some("连接已中断，此 Key 未发送；请重新检查后操作");
                    }
                    Some(format!("{error:#}"))
                }
                Err(_) => {
                    stopped = Some("批量删除超时，此 Key 未发送；请刷新检查后操作");
                    Some("等待删除结果超时，是否删除请刷新检查".into())
                }
            }
        };
        if let Some(error) = failure {
            failed += 1;
            // A server may return an arbitrarily long error or key. Bound the report as well
            // as the number of operations; the original selection is not duplicated here.
            let mut key = key;
            truncate(&mut key, 512);
            let mut error = error;
            truncate(&mut error, 1024);
            let bytes = key.len() + error.len();
            if failures.len() < 50 && retained + bytes <= 32 * 1024 {
                retained += bytes;
                failures.push((key, error));
            }
        }
    }
    if failed == 0 {
        Ok(Output::Changed {
            key: None,
            message: "Key 已删除".into(),
        })
    } else {
        Ok(Output::DeleteReport {
            succeeded,
            failed,
            failures,
        })
    }
}

fn truncate(text: &mut String, limit: usize) {
    if text.len() > limit {
        let mut end = limit.saturating_sub('…'.len_utf8());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push('…');
        text.shrink_to_fit();
    }
}
