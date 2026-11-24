use super::*;
use tomato_services::{
    commands::Output as CommandOutput,
    files::{document::Action, export},
};
#[tokio::test]
async fn live_file_actor_edits_markdown_without_consuming_browser_replies_and_exports_with_raii()
-> Result<()> {
    let root = tempfile::tempdir()?;
    let path = root.path().join("文档.md");
    tokio::fs::write(&path, "# 原文").await?;
    image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
        .save(root.path().join("图.png"))?;
    let connection = connection(root.path());
    let before = serde_json::to_vec(&connection)?;
    let mut session = FileSession::start(&tokio::runtime::Handle::current(), connection.clone());
    tokio::time::timeout(Duration::from_secs(5), async {
        while session.status() == Status::Connecting {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(session.status(), Status::Ready);
    let listing = session.request(Operation::List(remote(root.path())))?;
    let client = session.document_client();
    assert!(
        matches!(client.execute(Action::Image { document: remote(&path), source: "%E5%9B%BE.png".into() }).await?, CommandOutput::MarkdownImage(picture) if picture.width == 2 && picture.height == 2)
    );
    assert!(matches!(client.execute(Action::Image {
        document: remote(&path),
        source: url::Url::from_file_path(root.path().join("图.png")).unwrap().into(),
    }).await?, CommandOutput::MarkdownImage(picture) if picture.rgba == [1, 2, 3, 255].repeat(4)));
    assert!(
        matches!(client.execute(Action::Read(remote(&path))).await?, CommandOutput::Text(text) if text == "# 原文")
    );
    client
        .execute(Action::Write {
            path: remote(&path),
            text: "# 已保存\n\n完整正文".into(),
        })
        .await?;
    assert_eq!(
        tokio::fs::read_to_string(&path).await?,
        "# 已保存\n\n完整正文"
    );
    let reply = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(reply) = session.try_reply() {
                break reply;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    assert_eq!(reply.id, listing);
    assert!(matches!(reply.result, Ok(Output::Directory { entries, .. }) if entries.len() == 2));
    let Output::ExportReady(exported, asset) = client
        .request(Operation::ExportFile(export::Request::new(remote(&path))))
        .await?
    else {
        anyhow::bail!("expected staged export")
    };
    assert_eq!(exported, remote(&path));
    assert_eq!(
        tokio::fs::read_to_string(asset.path()).await?,
        "# 已保存\n\n完整正文"
    );
    let temporary = asset.directory().to_owned();
    drop(asset);
    assert!(!temporary.exists());
    let cancelled = export::Request::new(remote(&path));
    cancelled.cancel.cancel();
    assert!(
        client
            .request(Operation::ExportFile(cancelled))
            .await
            .is_err()
    );
    assert!(
        client
            .request(Operation::ExportFile(export::Request::new(remote(
                root.path()
            ))))
            .await
            .is_err()
    );
    session.stop().await;
    assert!(client.execute(Action::Read(remote(&path))).await.is_err());
    assert_eq!(serde_json::to_vec(&connection)?, before);
    assert!(path.exists());
    Ok(())
}
