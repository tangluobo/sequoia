use super::*;
use crate::app::tests::native_bridge::Bridge;
use tomato_core::{
    backup::{Kind, Scope, Selection},
    workbench::TableId,
};
fn ready(ctx: &egui::Context) -> Draft {
    let mut connection = Connection::new("MYSQL");
    connection.name = "fixture".into();
    let mut draft = Draft::new(
        connection,
        Scope {
            connection: "fixture".into(),
            database: "sample_db".into(),
            path: String::new(),
        },
        ctx,
    );
    draft.loading = false;
    draft.groups = selection::groups(
        ["users", "orders"]
            .map(|name| Selection {
                kind: Kind::Table,
                table: TableId {
                    schema: "sample_db".into(),
                    name: name.into(),
                },
            })
            .into(),
    );
    draft
}
#[test]
fn native_tree_checkbox_arrows_keyboard_and_scrollbar_keep_the_selected_object()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let bridge = Bridge::new(&ctx);
    let mut draft = ready(&ctx);
    draft.tab = 1;
    draft.groups = selection::groups(
        (0..50)
            .map(|index| Selection {
                kind: Kind::Table,
                table: TableId {
                    schema: "sample_db".into(),
                    name: format!("item-{index:03}"),
                },
            })
            .collect(),
    );
    app.backup_dialog = Some(draft);
    bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
    bridge.click(&ctx, egui::pos2(51., 184.), |ctx| app.backup_dialog(ctx));
    assert!(app.backup_dialog.as_ref().unwrap().groups[0].items[0].selected);
    let text = bridge.text("item-000")?;
    bridge.click(&ctx, text, |ctx| app.backup_dialog(ctx));
    bridge.frame(
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        |ctx| app.backup_dialog(ctx),
    );
    assert_eq!(
        app.backup_dialog.as_ref().unwrap().tree_selected,
        Some((Kind::Table, 4))
    );
    bridge.frame(
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::ArrowDown,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Default::default(),
        }],
        |ctx| app.backup_dialog(ctx),
    );
    bridge.click(&ctx, egui::pos2(713., 476.), |ctx| app.backup_dialog(ctx));
    assert!(app.backup_dialog.as_ref().unwrap().tree_scroll >= 26.);
    assert!(app.backup_dialog.as_ref().unwrap().groups[0].items[0].selected);
    let first = bridge.text("item-000")?;
    bridge.click(&ctx, first, |ctx| app.backup_dialog(ctx));
    bridge.frame(
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::End,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
        |ctx| app.backup_dialog(ctx),
    );
    assert_eq!(
        app.backup_dialog.as_ref().unwrap().tree_selected,
        Some((Kind::Event, 2))
    );
    assert!(app.backup_dialog.as_ref().unwrap().tree_scroll > 1000.);
    assert_eq!(
        app.backup_dialog.as_ref().unwrap().objects()[0].table.name,
        "item-000"
    );
    assert!(app.pending.is_empty());
    Ok(())
}
#[test]
fn real_template_menu_roundtrips_selected_objects_preserves_unknown_settings_and_close_cancels()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let global = directory.path().join("global.json");
    std::fs::write(&global, br#"{"future":{"keep":42}}"#)?;
    let ctx = egui::Context::default();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    let bridge = Bridge::new(&ctx);
    let mut draft = ready(&ctx);
    draft.options.comment = "保存的注释".into();
    draft.options.single_transaction = true;
    draft.groups[0].set_item(1, true);
    app.backup_dialog = Some(draft);
    bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
    bridge.click(&ctx, egui::pos2(485., 556.), |ctx| app.backup_dialog(ctx));
    let save = bridge.text("保存为模板")?;
    bridge.click(&ctx, save, |ctx| app.backup_dialog(ctx));
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&global)?)?;
    assert_eq!(saved["future"]["keep"], 42);
    assert_eq!(saved[TEMPLATE]["comment"], "保存的注释");
    assert_eq!(saved[TEMPLATE]["groups"][0]["selected"][0], "orders");
    if let Some(draft) = app.backup_dialog.take() {
        draft.release(&ctx);
    }
    app.backup_dialog = Some(ready(&ctx));
    bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
    bridge.click(&ctx, egui::pos2(485., 556.), |ctx| app.backup_dialog(ctx));
    let load = bridge.text("加载模板")?;
    bridge.click(&ctx, load, |ctx| app.backup_dialog(ctx));
    let draft = app.backup_dialog.as_ref().unwrap();
    assert_eq!(draft.options.comment, "保存的注释");
    assert!(draft.options.single_transaction);
    assert_eq!(draft.objects().len(), 1);
    assert_eq!(draft.objects()[0].table.name, "orders");
    bridge.click(&ctx, egui::pos2(485., 556.), |ctx| app.backup_dialog(ctx));
    let clear = bridge.text("清除模板")?;
    bridge.click(&ctx, clear, |ctx| app.backup_dialog(ctx));
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(&global)?)?;
    assert!(saved.get(TEMPLATE).is_none());
    assert_eq!(saved["future"]["keep"], 42);
    // Dismiss the success notice, then close through the real native button.
    bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
    let ok = bridge.text("确定")?;
    bridge.click(&ctx, ok, |ctx| app.backup_dialog(ctx));
    let draft = app.backup_dialog.as_mut().unwrap();
    draft.running = true;
    let control = draft.control.clone();
    bridge.click(&ctx, egui::pos2(675., 556.), |ctx| app.backup_dialog(ctx));
    assert!(app.backup_dialog.is_none());
    assert!(control.cancelled());
    assert!(app.pending.is_empty());
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
#[test]
fn template_loaded_during_catalog_request_applies_after_objects_arrive_and_legacy_options_still_load()
-> anyhow::Result<()> {
    let ctx = egui::Context::default();
    let mut source = ready(&ctx);
    source.groups[0].set_all(true);
    let template = template::Template::capture(&source);
    let mut destination = ready(&ctx);
    destination.loading = true;
    template.apply(&mut destination);
    assert!(destination.objects().is_empty());
    assert!(destination.template_groups.is_some());
    destination.loading = false;
    template::apply_groups(&mut destination);
    assert_eq!(destination.objects().len(), 2);
    let old: template::Template = serde_json::from_value(
        serde_json::json!({"comment":"old","lock_tables":false,"single_transaction":true,"custom_filename":false,"filename":""}),
    )?;
    old.apply(&mut destination);
    assert_eq!(destination.options.comment, "old");
    assert_eq!(destination.objects().len(), 2);
    Ok(())
}
