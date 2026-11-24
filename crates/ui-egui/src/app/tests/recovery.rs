//! Actual completion delivery and controls, using an executor that never opens sockets.
use super::*;
use tomato_services::database::{
    ConnectionFailure, FailureStage, RecoveryState,
    script::{Interrupted, Interruption, Mode, Phase, Script, Statement},
};
fn application() -> anyhow::Result<(tempfile::TempDir, egui::Context, TomatoApp)> {
    let directory = tempfile::tempdir()?;
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let app = TomatoApp::with_repository(
        &ctx,
        runtime,
        ConnectionFileRepository::new(directory.path().join("connections.json")),
    );
    Ok((directory, ctx, app))
}
fn failure(
    connection: &Connection,
    recovery: RecoveryState,
    stage: FailureStage,
) -> ConnectionFailure {
    ConnectionFailure {
        connection_id: connection.id.clone(),
        transaction_lost: false,
        attempts: 2,
        read_only: false,
        stage,
        recovery,
    }
}
#[test]
fn interrupted_script_keeps_owned_rows_and_execution_context_without_duplicate_errors()
-> anyhow::Result<()> {
    for restored in [false, true] {
        let (_directory, ctx, mut app) = application()?;
        let mut connection = Connection::new("MYSQL");
        connection.set("database", "original_db");
        let tab = crate::tabs::create_tab(connection.clone(), None)?;
        let id = tab.id.clone();
        app.tabs.push(tab);
        app.active = Some(id.clone());
        let sql = "SELECT '保留结果'; KILL 7; INSERT INTO items VALUES ('never run')";
        app.submit(
            &id,
            Command::SqlScript {
                connection: connection.clone(),
                sql: sql.into(),
                mode: Mode::Run,
            },
            Purpose::Normal,
        );
        let task = *app.pending.keys().next().unwrap();
        let failure = failure(
            &connection,
            if restored {
                RecoveryState::Restored
            } else {
                RecoveryState::Failed
            },
            FailureStage::Executing,
        );
        let result = tomato_services::QueryResult {
            columns: vec!["value".into()],
            rows: vec![vec!["保留结果".into()]],
            has_result_set: true,
            total_rows: 1,
            ..Default::default()
        };
        let address = result.rows[0][0].as_ptr();
        let mut script = Script {
            source: sql.into(),
            results: vec![result],
            success_count: 1,
            fail_count: 1,
            interruption: Some(Interruption {
                phase: Phase::Statement,
                message: failure.to_string(),
                connection: Some(failure.clone()),
            }),
            ..Default::default()
        };
        script.statements.push(Statement {
            range: 0.."SELECT '保留结果'".len(),
            select: true,
            source_table: None,
            elapsed_ms: 1,
            success: true,
            error: None,
            result: Some(0),
            has_result_set: true,
            row_count: 1,
            affected: None,
            truncated: false,
            editing: None,
            edit_error: None,
        });
        script.statements.push(Statement {
            range: sql.find("KILL").unwrap()..sql.find("; INSERT").unwrap(),
            select: false,
            source_table: None,
            elapsed_ms: 1,
            success: false,
            error: Some("socket closed".into()),
            result: None,
            has_result_set: false,
            row_count: 0,
            affected: None,
            truncated: false,
            editing: None,
            edit_error: None,
        });
        let error = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "fixture",
        ))
        .context(failure)
        .context(Interrupted {
            script: Box::new(script),
        });
        app.queue.deliver_for_test(task, Err(error));
        app.logic(&ctx);
        let tab = &mut app.tabs[0];
        assert!(!tab.busy);
        assert!(tab.error.is_none());
        let Panel::Sql {
            results, workbench, ..
        } = &mut tab.panel
        else {
            unreachable!()
        };
        assert_eq!(results[0].rows[0][0], "保留结果");
        assert_eq!(
            results[0].rows[0][0].as_ptr(),
            address,
            "Completion cloned a retained row"
        );
        assert_eq!(
            workbench.result_context.as_ref().unwrap().connection.id,
            connection.id
        );
        assert_eq!(workbench.script_view, 0);
        assert_eq!(workbench.failure.is_none(), restored);
        let script = workbench.script.as_ref().unwrap();
        assert_eq!(script.statements.len(), 2);
        assert_eq!(script.success_count, 1);
        assert_eq!(script.fail_count, 1);
        assert!(script.info().contains("保留结果"));
        assert!(
            script.results.is_empty(),
            "Result buffer was retained twice"
        );
        assert!(
            app.error.is_none(),
            "The generic egui floating window must not duplicate the native alert"
        );
        assert!(!app.status.is_empty());
        assert_eq!(app.root_modal_open(), !restored);
        if !restored {
            let bridge = super::native_bridge::Bridge::new(&ctx);
            bridge.root_frame(&ctx, vec![], vec![], |ui| app.ui(ui));
            bridge.text("确定")?;
            bridge.root_frame(
                &ctx,
                vec![],
                vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                |ui| app.ui(ui),
            );
            let Panel::Sql {
                workbench, results, ..
            } = &app.tabs[0].panel
            else {
                unreachable!()
            };
            assert!(workbench.failure.is_none());
            assert_eq!(results[0].rows[0][0], "保留结果");
        }
    }
    Ok(())
}
#[test]
fn connection_failures_use_one_notice_and_preserve_unrelated_namespace_transactions()
-> anyhow::Result<()> {
    for kind in 0..3 {
        for preserved in [false, true] {
            let (_directory, ctx, mut app) = application()?;
            let connection = Connection::new("MYSQL");
            let mut tab = crate::tabs::create_tab(connection.clone(), None)?;
            let id = tab.id.clone();
            let Panel::Sql { workbench, .. } = &mut tab.panel else {
                unreachable!()
            };
            workbench.transaction = true;
            if kind == 1 {
                workbench.browser = Some(Box::new(
                    crate::tabs::database_objects::window::Draft::default(),
                ));
            }
            if kind == 2 {
                workbench.designer = Some(Box::new(crate::tabs::table_design::Draft::new(
                    "MYSQL",
                    "owned".into(),
                )?));
            }
            app.tabs.push(tab);
            app.active = Some(id.clone());
            app.submit(
                &id,
                Command::SqlScript {
                    connection: connection.clone(),
                    sql: "SELECT 1".into(),
                    mode: Mode::Run,
                },
                Purpose::Normal,
            );
            let task = *app.pending.keys().next().unwrap();
            let failure = failure(
                &connection,
                if preserved {
                    RecoveryState::Preserved
                } else {
                    RecoveryState::Failed
                },
                FailureStage::Opening,
            );
            app.queue.deliver_for_test(
                task,
                Err(anyhow::Error::new(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "raw os error 10053",
                ))
                .context(failure)),
            );
            app.logic(&ctx);
            let tab = &app.tabs[0];
            assert!(!tab.busy);
            assert!(tab.error.is_none());
            let Panel::Sql { workbench, .. } = &tab.panel else {
                unreachable!()
            };
            assert_eq!(workbench.transaction, preserved);
            assert_eq!(workbench.failure.is_some(), kind == 0);
            let mut notices = usize::from(workbench.failure.is_some());
            if let Some(browser) = &workbench.browser {
                assert!(
                    browser
                        .notice
                        .as_ref()
                        .is_some_and(|text| text.contains("尚未发送"))
                );
                notices += usize::from(browser.notice.is_some());
            }
            if let Some(designer) = &workbench.designer {
                assert!(
                    designer
                        .notice
                        .as_ref()
                        .is_some_and(|notice| notice.text.contains("尚未发送"))
                );
                notices += usize::from(designer.notice.is_some());
                assert!(!designer.busy);
                assert!(!designer.saving);
            }
            assert_eq!(notices, 1);
            assert!(app.error.is_none());
            assert!(app.root_modal_open());
            assert!(!app.status.contains("10053"));
        }
    }
    Ok(())
}
