use super::*;
#[tokio::test]
async fn image_double_click_reads_real_actor_then_downloads_and_deletes_actual_object()
-> anyhow::Result<()> {
    use crate::tabs::ssh::images::PreviewAction;
    let fixture = Fixture::start().await?;
    let mut encoded = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        64,
        32,
        image::Rgba([70, 130, 180, 255]),
    ))
    .write_to(&mut encoded, image::ImageFormat::Png)?;
    let bytes = encoded.into_inner();
    fixture
        .data
        .lock()
        .unwrap()
        .insert(("source".into(), "image.png".into()), bytes.clone());
    let connection = fixture.connection("source");
    let mut terminal = tomato_services::terminal::Terminal::from_file_session(FileSession::start(
        &tokio::runtime::Handle::current(),
        connection.clone(),
    ));
    let mut workspace = Workspace::standalone_files("ICON", "/");
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let mut icons = crate::icons::Icons::default();
    wait(&mut workspace, &mut terminal, &ctx, |view| {
        view.panes[&Section::Files].entries.len() == 2
    })
    .await?;
    assert!(
        workspace.panes[&Section::Files].columns.is_empty(),
        "the first root listing must not append an empty duplicate directory column"
    );
    workspace.enqueue(Section::Files, Operation::List("/".into()));
    wait(&mut workspace, &mut terminal, &ctx, |view| {
        !view.busy(Section::Files)
    })
    .await?;
    assert!(
        workspace.panes[&Section::Files].columns.is_empty(),
        "refreshing the current root must not create duplicate columns"
    );
    let texts = frame(&ctx, &mut workspace, &connection, &mut icons, vec![]);
    let point = texts
        .iter()
        .find(|(text, _)| text == "image.png")
        .expect("actual image row")
        .1;
    click(&ctx, &mut workspace, &connection, &mut icons, point);
    click(&ctx, &mut workspace, &connection, &mut icons, point);
    assert_eq!(
        workspace.previews.len(),
        1,
        "double click must open the image viewer"
    );
    wait(&mut workspace, &mut terminal, &ctx, |view| {
        view.previews[0].texture.is_some()
    })
    .await
    .map_err(|error| anyhow::anyhow!("read image: {error}; {}", workspace.previews[0].error))?;
    assert_eq!(workspace.previews[0].dimensions, egui::vec2(64., 32.));
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("downloaded.png");
    workspace.previews[0].action = Some(PreviewAction::Download(path.clone()));
    wait(&mut workspace, &mut terminal, &ctx, |view| {
        view.previews[0].notice.is_some()
    })
    .await
    .map_err(|error| anyhow::anyhow!("download image: {error}"))?;
    assert_eq!(std::fs::read(path)?, bytes);
    assert_eq!(workspace.previews[0].notice.as_ref().unwrap().0, "下载完成");
    workspace.previews[0].notice = None;
    workspace.previews[0].action = Some(PreviewAction::Delete);
    wait(&mut workspace, &mut terminal, &ctx, |view| {
        view.previews.is_empty()
            && !view.panes[&Section::Files]
                .entries
                .iter()
                .any(|entry| entry.name == "image.png")
    })
    .await
    .map_err(|error| {
        anyhow::anyhow!(
            "delete image: {error}; remaining previews={}, notice={:?}",
            workspace.previews.len(),
            workspace
                .previews
                .first()
                .and_then(|preview| preview.notice.as_ref())
        )
    })?;
    assert!(
        !fixture
            .data
            .lock()
            .unwrap()
            .contains_key(&("source".into(), "image.png".into()))
    );
    terminal.stop_file_session().await;
    Ok(())
}
