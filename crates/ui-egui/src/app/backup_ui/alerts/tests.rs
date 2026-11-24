use super::super::*;
use crate::app::tests::native_bridge::Bridge;

#[test]
fn native_success_acknowledgement_closes_wizard_but_failure_keeps_options_and_logs()
-> anyhow::Result<()> {
    for success in [false, true] {
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
        let mut draft = Draft::new(
            Connection::new("MYSQL"),
            tomato_core::backup::Scope {
                connection: "fixture".into(),
                database: "sample_db".into(),
                path: String::new(),
            },
            &ctx,
        );
        draft.loading = false;
        draft.options.comment = "保留的备份注释".into();
        draft.control.log("保留的任务日志");
        if success {
            draft.complete = Some("备份文件已保存:\nfixture.nb3".into());
        } else {
            draft.error = Some("错误: 本地测试失败".into());
            draft.notice = draft.error.clone();
        }
        let parent = egui::ViewportId::from_hash_of(("backup-dialog", &draft.id));
        app.backup_dialog = Some(draft);
        bridge.frame(&ctx, Vec::new(), |ctx| app.backup_dialog(ctx));
        bridge.close_viewport(&ctx, parent, |ctx| app.backup_dialog(ctx));
        assert!(
            app.backup_dialog.is_some(),
            "a blocked owner cannot bypass its modal alert"
        );
        let ok = bridge.text("确定")?;
        bridge.click(&ctx, ok, |ctx| app.backup_dialog(ctx));
        if success {
            assert!(app.backup_dialog.is_none());
        } else {
            let draft = app.backup_dialog.as_ref().unwrap();
            assert!(draft.notice.is_none());
            assert_eq!(draft.error.as_deref(), Some("错误: 本地测试失败"));
            assert_eq!(draft.options.comment, "保留的备份注释");
            assert!(!draft.running);
        }
        assert!(app.pending.is_empty());
        assert!(!directory.path().join("connections.json").exists());
    }
    Ok(())
}
