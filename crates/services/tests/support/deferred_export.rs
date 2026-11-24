use anyhow::Result;
use std::{future::Future, time::Duration};
use tomato_services::files::{export::Request, operations::Output};

pub async fn assert_after_drop(
    request: Request,
    operation: impl Future<Output = Result<Output>>,
    expected: &[u8],
    child: &str,
) -> Result<()> {
    let output = {
        tokio::pin!(operation);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                tokio::select! {
                    _ = operation.as_mut() => panic!("transfer finished before drop acceptance"),
                    _ = tokio::time::sleep(Duration::from_millis(2)) => {
                        if request.started() { break; }
                    }
                }
            }
        })
        .await?;
        assert!(request.started());
        assert_eq!(
            request.items().len(),
            2,
            "hover retains only the selected roots"
        );
        request.accept_drop();
        tokio::time::timeout(Duration::from_secs(20), operation).await??
    };
    assert!(request.items().iter().any(|item| item.path == child));
    assert!(
        request.items().len() > 2,
        "accepted drop expands the directory through its real adapter"
    );
    assert!(request.items().iter().any(|item| item.directory));
    assert!(request.failure().is_none());
    let Output::ExportReady(_, asset) = output else {
        anyhow::bail!("missing deferred export")
    };
    assert_eq!(tokio::fs::read(asset.path()).await?, expected);
    let staged = asset.directory().to_owned();
    drop(request);
    drop(asset);
    assert!(
        !staged.exists(),
        "the final export lease releases its temporary files"
    );
    Ok(())
}
