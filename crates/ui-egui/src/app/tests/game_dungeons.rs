use super::*;
use crate::tabs::game_dungeons::State;
use chrono::Datelike;

fn app(path: &std::path::Path) -> anyhow::Result<TomatoApp> {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    Ok(TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(path.join("connections.json")),
    ))
}
fn frame(app: &mut TomatoApp, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.context.clone();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200., 900.),
            )),
            events,
            ..Default::default()
        },
        |ui| app.ui(ui),
    );
    output.textures_delta.clear();
    output
}
fn point(app: &mut TomatoApp, label: &str) -> anyhow::Result<egui::Pos2> {
    // A newly opened modal receives its measured position on its second frame.
    frame(app, vec![]);
    frame(app, vec![])
        .shapes
        .iter()
        .rev()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("missing dungeon control {label}"))
}
fn click_at(app: &mut TomatoApp, pos: egui::Pos2) {
    for pressed in [true, false] {
        frame(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
}
fn click(app: &mut TomatoApp, label: &str) -> anyhow::Result<()> {
    let pos = point(app, label)?;
    click_at(app, pos);
    Ok(())
}
fn type_below(app: &mut TomatoApp, label: &str, text: &str) -> anyhow::Result<()> {
    let pos = point(app, label)?;
    click_at(app, pos + egui::vec2(0., 24.));
    frame(app, vec![egui::Event::Text(text.into())]);
    Ok(())
}
fn state(app: &TomatoApp) -> &State {
    let Panel::Tool(tool) = &app.tabs[0].panel else {
        panic!("tool")
    };
    tool.game_dungeons.as_ref().unwrap()
}
fn wait(app: &mut TomatoApp) -> anyhow::Result<()> {
    let started = std::time::Instant::now();
    loop {
        app.logic(&app.context.clone());
        frame(app, vec![]);
        if app.pending.is_empty() && !app.tabs[0].busy && state(app).snapshot.is_some() {
            break;
        }
        anyhow::ensure!(started.elapsed().as_secs() < 15, "dungeon command timeout");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    anyhow::ensure!(app.tabs[0].error.is_none(), "{:?}", app.tabs[0].error);
    Ok(())
}
fn square(app: &mut TomatoApp, color: egui::Color32) -> anyhow::Result<egui::Pos2> {
    frame(app, vec![])
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.fill == color && rect.rect.size() == egui::vec2(34., 34.) =>
            {
                Some(rect.rect.center())
            }
            _ => None,
        })
        .ok_or_else(|| anyhow::anyhow!("missing completion square {color:?}"))
}
#[test]
fn actual_relaxation_picker_forms_completion_reload_and_delete_use_isolated_commands()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    Store::default().save(&directory.path().join("connections.json"))?;
    let original = std::fs::read(directory.path().join("connections.json"))?;
    let mut app = app(directory.path())?;
    app.picker = Some(5);
    click(&mut app, "游戏副本管理")?;
    assert!(app.dialog.is_none() && app.picker.is_none());
    frame(&mut app, vec![]);
    wait(&mut app)?;
    assert_eq!(
        state(&app).path,
        directory.path().join("game-dungeons.json")
    );
    click(&mut app, "添加账号")?;
    type_below(&mut app, "账号名称（用于区分，无需密码）", "主账号")?;
    click(&mut app, "保存")?;
    wait(&mut app)?;
    assert_eq!(state(&app).board().unwrap().accounts[0].name, "主账号");
    click(&mut app, "添加角色")?;
    type_below(&mut app, "角色名称", "剑客")?;
    click(&mut app, "保存")?;
    wait(&mut app)?;
    assert_eq!(state(&app).board().unwrap().characters[0].name, "剑客");
    click(&mut app, "添加副本")?;
    type_below(&mut app, "副本名称", "团本一")?;
    click(&mut app, "周四")?;
    click(&mut app, "周一")?;
    click(&mut app, "保存")?;
    wait(&mut app)?;
    assert_eq!(
        state(&app).board().unwrap().dungeons[0].reset_day,
        tomato_core::game_dungeons::Weekday::Monday
    );
    let red = egui::Color32::from_rgb(211, 64, 64);
    let green = egui::Color32::from_rgb(20, 151, 83);
    let pos = square(&mut app, red)?;
    click_at(&mut app, pos);
    wait(&mut app)?;
    square(&mut app, green)?;
    let today = chrono::Local::now().date_naive().num_days_from_ce();
    let board = state(&app).board().unwrap();
    assert!(board.completed(&board.characters[0].id, &board.dungeons[0], today));
    app.tabs.clear();
    app.active = None;
    app.open_tool("game_dungeons", "游戏副本管理");
    frame(&mut app, vec![]);
    wait(&mut app)?;
    let pos = square(&mut app, green)?;
    click_at(&mut app, pos);
    wait(&mut app)?;
    square(&mut app, red)?;
    click(&mut app, "剑客")?;
    click(&mut app, "删除")?;
    click(&mut app, "取消")?;
    assert_eq!(state(&app).board().unwrap().characters.len(), 1);
    click(&mut app, "剑客")?;
    click(&mut app, "删除")?;
    click(&mut app, "确认删除")?;
    wait(&mut app)?;
    assert!(state(&app).board().unwrap().characters.is_empty());
    assert_eq!(
        std::fs::read(directory.path().join("connections.json"))?,
        original
    );
    let shutdown = app.queue.shutdown();
    app.runtime.as_ref().unwrap().block_on(shutdown.wait());
    Ok(())
}
