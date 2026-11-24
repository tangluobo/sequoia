use super::*;
use crate::app::tree_rename::tests::Driver;
fn set_text(d: &mut Driver, id: &str, value: &str) {
    if let Panel::Editor { text, markdown, .. } =
        &mut d.app.tabs.iter_mut().find(|t| t.id == id).unwrap().panel
    {
        *text = value.into();
        markdown.changed(10.);
    }
}
fn file_operation(
    d: &mut Driver,
    owner: &str,
    operation: tomato_services::files::operations::Operation,
) {
    if let Panel::Terminal { view, .. } =
        &mut d.app.tabs.iter_mut().find(|t| t.id == owner).unwrap().panel
    {
        view.ssh.as_mut().unwrap().enqueue(
            tomato_services::terminal::ssh::tools::Section::Files,
            operation,
        );
    }
}
fn wait(d: &mut Driver, ready: impl Fn(&Driver) -> bool) -> anyhow::Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    loop {
        d.app.logic(&d.app.context.clone());
        if ready(d) {
            return Ok(());
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "owned document operation timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
fn browser_owner(d: &mut Driver) -> anyhow::Result<String> {
    let tab = crate::tabs::create_tab(d.local.clone(), d.app.runtime.as_ref().map(|r| r.handle()))?;
    let owner = tab.id.clone();
    d.app.tabs.push(tab);
    d.app.active = Some(owner.clone());
    Ok(owner)
}
#[test]
fn file_browser_markdown_opens_previews_saves_and_retains_draft_after_owner_disconnect()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let owner = browser_owner(&mut d)?;
    let path = d.directory.path().join("files/first.md");
    let config = std::fs::read(d.directory.path().join("connections.json"))?;
    d.app.handle_intent(
        &owner,
        Intent::OpenFileDocument(path.to_string_lossy().replace('\\', "/")),
    );
    d.drain()?;
    let id = d.app.active.clone().unwrap();
    let revision = match &d.app.tabs.iter().find(|t| t.id == id).unwrap().panel {
        Panel::Editor { text, markdown, .. } => {
            assert_eq!(text, "# actual bytes");
            assert!(markdown.file_client.is_some());
            markdown.revision
        }
        _ => panic!("real editor missing"),
    };
    d.app.handle_intent(
        &id,
        Intent::Markdown {
            source: "# actual bytes".into(),
            revision,
        },
    );
    d.drain()?;
    assert!(
        matches!(&d.app.tabs.iter().find(|t|t.id==id).unwrap().panel,Panel::Editor{markdown,..}if !markdown.document.blocks.is_empty())
    );
    set_text(&mut d, &id, "# 保存的正文");
    d.app
        .handle_intent(&id, Intent::Save(path.clone(), "# 保存的正文".into()));
    d.drain()?;
    assert_eq!(std::fs::read_to_string(&path)?, "# 保存的正文");
    let renamed = path.with_file_name("renamed.md");
    file_operation(
        &mut d,
        &owner,
        tomato_services::files::operations::Operation::Rename {
            from: path.to_string_lossy().replace('\\', "/"),
            to: renamed.to_string_lossy().replace('\\', "/"),
        },
    );
    wait(
        &mut d,
        |d| matches!(&d.app.tabs.iter().find(|t|t.id==id).unwrap().panel,Panel::Editor{path,..}if path==&renamed),
    )?;
    assert!(!path.exists());
    let path = renamed;
    let created = path.with_file_name("created.md");
    file_operation(
        &mut d,
        &owner,
        tomato_services::files::operations::Operation::Create(
            created.to_string_lossy().replace('\\', "/"),
        ),
    );
    wait(&mut d, |d| {
        d.app
            .tabs
            .iter()
            .any(|t| matches!(&t.panel,Panel::Editor{path,..}if path==&created))
    })?;
    d.drain()?;
    assert_eq!(std::fs::read_to_string(&created)?, "");
    d.app.close(&owner);
    std::thread::sleep(std::time::Duration::from_millis(100));
    set_text(&mut d, &id, "# 断线草稿");
    d.app
        .handle_intent(&id, Intent::Save(path.clone(), "# 断线草稿".into()));
    d.drain()?;
    assert!(
        d.app
            .tabs
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .error
            .is_some()
    );
    assert_eq!(std::fs::read_to_string(&path)?, "# 保存的正文");
    assert!(
        matches!(&d.app.tabs.iter().find(|t|t.id==id).unwrap().panel,Panel::Editor{text,..}if text=="# 断线草稿")
    );
    let mut reopened =
        crate::tabs::create_tab(d.local.clone(), d.app.runtime.as_ref().map(|r| r.handle()))?;
    reopened.id = owner.clone();
    d.app.tabs.push(reopened);
    d.app
        .open_file_document(&owner, path.to_string_lossy().replace('\\', "/"));
    set_text(&mut d, &id, "# 重连保存");
    d.app
        .handle_intent(&id, Intent::Save(path.clone(), "# 重连保存".into()));
    d.drain()?;
    assert_eq!(std::fs::read_to_string(&path)?, "# 重连保存");
    assert_eq!(
        std::fs::read(d.directory.path().join("connections.json"))?,
        config
    );
    Ok(())
}

#[test]
fn nested_file_editors_keep_browse_tab_and_cancelled_connection_close_preserves_unsaved_text()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let owner = browser_owner(&mut d)?;
    // Use the owned real filesystem actor behind an SFTP-shaped UI fixture.
    if let Panel::Terminal { connection, .. } =
        &mut d.app.tabs.iter_mut().find(|t| t.id == owner).unwrap().panel
    {
        connection.kind = Some("SFTP".into());
    }
    let path = d.directory.path().join("files/first.md");
    d.app
        .open_file_document(&owner, path.to_string_lossy().replace('\\', "/"));
    d.drain()?;
    let id = d
        .app
        .tabs
        .iter()
        .find(|t| matches!(&t.panel,Panel::Editor{markdown,..}if markdown.file_embedded))
        .unwrap()
        .id
        .clone();
    assert_eq!(d.app.active.as_deref(), Some(owner.as_str()));
    assert!(
        !d.app
            .tabs
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .workspace_visible()
    );
    fn draw(d: &mut Driver, owner: &str, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
        let context = d.app.context.clone();
        let mut intent = None;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1050., 760.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                if let Some(index) = d.app.tabs.iter().position(|t| t.id == owner) {
                    intent = d.app.render_file_documents(ui, index).unwrap().intent;
                }
                d.app.confirm_dialog(&context);
            },
        );
        if let Some((id, intent)) = intent {
            d.app.handle_intent(&id, intent);
        }
        output.textures_delta.clear();
        output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => {
                    Some((t.galley.job.text.clone(), t.pos + t.galley.size() / 2.))
                }
                _ => None,
            })
            .collect()
    }
    let labels = draw(&mut d, &owner, vec![]);
    d.drain()?;
    assert!(labels.iter().any(|(s, _)| s == "浏览"));
    set_text(&mut d, &id, "# 保留未保存内容");
    d.app.close(&owner);
    assert!(matches!(&d.app.confirmation,Some(Confirmation::Close(target))if target==&id));
    draw(&mut d, &owner, vec![]);
    let cancel = draw(&mut d, &owner, vec![])
        .into_iter()
        .find(|(s, _)| s == "取消")
        .unwrap()
        .1;
    for pressed in [true, false] {
        draw(
            &mut d,
            &owner,
            vec![
                egui::Event::PointerMoved(cancel),
                egui::Event::PointerButton {
                    pos: cancel,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    d.app.continue_file_owner_close();
    assert!(d.app.tabs.iter().any(|t| t.id == owner));
    assert!(d.app.file_owner_close.is_none());
    assert!(
        matches!(&d.app.tabs.iter().find(|t|t.id==id).unwrap().panel,Panel::Editor{text,..}if text=="# 保留未保存内容")
    );
    set_text(&mut d, &id, "# actual bytes");
    d.app.close(&owner);
    assert!(!d.app.tabs.iter().any(|t| t.id == owner || t.id == id));
    Ok(())
}
