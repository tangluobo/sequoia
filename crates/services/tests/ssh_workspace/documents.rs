use super::*;
#[cfg(windows)]
#[path = "../support/markdown_print.rs"]
mod native_print;
use tomato_services::{
    commands::{Command, CommandHandler, Engine, Output as CommandOutput},
    files::{document::Action, export},
};
#[tokio::test]
async fn markdown_and_drag_export_share_the_authenticated_sftp_session() -> Result<()> {
    let fixture = Fixture::start(Mode::Shell).await?;
    let mut terminal = fixture.terminal("SFTP").await?;
    tokio::fs::write(fixture.root.path().join("文档.md"), "# 原文").await?;
    let client = terminal.document_client();
    assert!(
        matches!(client.execute(Action::Read("/文档.md".into())).await?, CommandOutput::Text(text) if text == "# 原文")
    );
    client
        .execute(Action::Write {
            path: "/文档.md".into(),
            text: "# 已保存".into(),
        })
        .await?;
    assert_eq!(
        tokio::fs::read_to_string(fixture.root.path().join("文档.md")).await?,
        "# 已保存"
    );
    let Output::ExportReady(_, asset) = client
        .request(Operation::ExportFile(export::Request::new(
            "/文档.md".into(),
        )))
        .await?
    else {
        anyhow::bail!("expected export")
    };
    assert_eq!(tokio::fs::read_to_string(asset.path()).await?, "# 已保存");
    assert!(
        matches!(request(&mut terminal, Operation::List("/".into())).await?, Output::Directory { entries, .. } if entries.iter().any(|e| e.name == "文档.md"))
    );
    let document = "/notes/文档.md";
    tokio::fs::create_dir(fixture.root.path().join("notes")).await?;
    tokio::fs::write(
        fixture.root.path().join("notes/文档.md"),
        "# SFTP 文档\n\n![图](../a%20b+图.png)",
    )
    .await?;
    image::RgbaImage::from_pixel(1800, 4, image::Rgba([9, 18, 27, 255]))
        .save(fixture.root.path().join("a b+图.png"))?;
    let CommandOutput::Text(source) = client.execute(Action::Read(document.into())).await? else {
        anyhow::bail!("missing Markdown source");
    };
    let local = tempfile::tempdir()?;
    let html = local.path().join("文档.md.html");
    Engine::default()
        .execute(Command::MarkdownHtml {
            source,
            title: "文档.md".into(),
            path: html.clone(),
        })
        .await?;
    let html = tokio::fs::read_to_string(html).await?;
    assert!(html.contains("<h1>SFTP 文档</h1>") && html.contains("<title>文档.md</title>"));
    assert!(matches!(client.execute(Action::Image {
        document: document.into(),
        source: " ..\\a%20b+%E5%9B%BE.png?ignored#ignored ".into(),
    }).await?, CommandOutput::MarkdownImage(picture) if (picture.width, picture.height) == (1600, 3) && picture.rgba[..4] == [9, 18, 27, 255]));
    drop(terminal);
    assert!(
        client
            .execute(Action::Read("/文档.md".into()))
            .await
            .is_err()
    );
    Ok(())
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires TOMATO_REMOTE_PRINT_EXE and a fresh TOMATO_REMOTE_PRINT_CAPTURE"]
async fn native_print_loads_remote_markdown_and_relative_images_over_sftp() -> Result<()> {
    let fixture = Fixture::start(Mode::Shell).await?;
    let root = fixture.root.path();
    std::fs::create_dir(root.join("notes"))?;
    std::fs::create_dir(root.join("images"))?;
    let document = root.join("notes/README.md");
    let source = "# SFTP remote print\n\nBEFORE ![Relative](../images/a%20b+%E5%9B%BE.png) AFTER\n\n| A | B |\n|---|---|\n| ROOT ![Root](/images/root.png) END | Text |\n";
    std::fs::write(&document, source)?;
    for (name, color) in [
        ("images/a b+图.png", [14, 101, 190, 255]),
        ("images/root.png", [13, 134, 79, 255]),
    ] {
        image::RgbaImage::from_pixel(56, 18, image::Rgba(color)).save(root.join(name))?;
    }
    let mut connection = Connection::new("SFTP");
    connection.set("host", "127.0.0.1");
    connection.set("port", fixture.port);
    connection.set("username", "fixture");
    connection.set("password", "fixture-only");
    native_print::capture(connection, "sftp-fixture").await?;
    assert_eq!(std::fs::read_to_string(document)?, source);
    Ok(())
}
