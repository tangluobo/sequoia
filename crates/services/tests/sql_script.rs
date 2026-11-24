use anyhow::Result;
use tomato_core::Connection;
use tomato_services::{
    commands::{Command, CommandHandler, Engine, Output},
    database::script::{Mode, Script},
};
async fn execute(
    engine: &mut Engine,
    connection: &Connection,
    sql: &str,
    mode: Mode,
) -> Result<Box<Script>> {
    let Output::Script(script) = engine
        .execute(Command::SqlScript {
            connection: connection.clone(),
            sql: sql.into(),
            mode,
        })
        .await?
    else {
        anyhow::bail!("script output required")
    };
    Ok(script)
}
#[tokio::test]
async fn sql_script_continues_after_error_retains_nulls_and_uses_the_same_transaction() -> Result<()>
{
    let mut engine = Engine::default();
    let connection = Connection::new("SQLITE");
    let sql = "CREATE TABLE items(id INTEGER PRIMARY KEY, value TEXT); BEGIN; INSERT INTO items VALUES(1,'semi;colon'); SELECT missing FROM items; SELECT id, value, NULL AS absent FROM items; ROLLBACK; SELECT COUNT(*) AS remaining FROM items";
    let script = execute(&mut engine, &connection, sql, Mode::Run).await?;
    assert_eq!((script.success_count, script.fail_count), (6, 1));
    assert_eq!(script.statements.len(), 7);
    assert_eq!(
        script.sql(&script.statements[3].range),
        "SELECT missing FROM items"
    );
    let result = &script.results[script.statements[4].result.unwrap()];
    assert_eq!(result.value(0, 1), Some("semi;colon"));
    assert_eq!(result.value(0, 2), None);
    assert_eq!(
        script.results[script.statements[6].result.unwrap()].value(0, 0),
        Some("0")
    );
    assert_eq!(script.plans.len(), 2);
    assert!(script.status.is_some());
    let info = script.info();
    assert!(info.contains("SELECT missing FROM items\n > 错误\n > "));
    assert!(info.contains(" > 行数: 1\n"));
    assert!(info.ends_with("成功: 6  失败: 1"));
    Ok(())
}
#[tokio::test]
async fn sql_script_explain_never_runs_original_writes_or_opens_a_session_for_non_select()
-> Result<()> {
    let mut engine = Engine::default();
    let connection = Connection::new("SQLITE");
    let empty = execute(
        &mut engine,
        &connection,
        "CREATE TABLE should_not_exist(id INTEGER)",
        Mode::Explain,
    )
    .await?;
    assert_eq!(empty.message.as_deref(), Some("没有可解释的SELECT语句"));
    assert!(empty.results.is_empty());
    let plan = execute(
        &mut engine,
        &connection,
        "CREATE TABLE should_not_exist(id INTEGER); SELECT 17 AS answer",
        Mode::Explain,
    )
    .await?;
    assert_eq!(plan.plans.len(), 1);
    assert!(plan.statements.is_empty());
    assert!(plan.status.is_none());
    assert!(!plan.results[0].rows.is_empty());
    let check = execute(
        &mut engine,
        &connection,
        "SELECT COUNT(*) FROM sqlite_master WHERE name='should_not_exist'",
        Mode::Run,
    )
    .await?;
    assert_eq!(
        check.results[check.statements[0].result.unwrap()].value(0, 0),
        Some("0")
    );
    let mut unreachable = Connection::new("MYSQL");
    unreachable.set("host", "127.0.0.1");
    unreachable.set("port", 0);
    assert!(
        execute(
            &mut engine,
            &unreachable,
            "UPDATE absent SET id=1",
            Mode::Explain
        )
        .await?
        .message
        .is_some()
    );
    for (kind, sql) in [
        (
            "MYSQL",
            "DELIMITER //\nCREATE PROCEDURE no_run() BEGIN SELECT 1; END//\nDELIMITER ;",
        ),
        (
            "POSTGRESQL",
            "CREATE FUNCTION no_run() RETURNS int LANGUAGE sql AS $$SELECT 1$$;",
        ),
    ] {
        unreachable.kind = Some(kind.into());
        assert!(
            execute(&mut engine, &unreachable, sql, Mode::Explain)
                .await?
                .message
                .is_some()
        );
    }
    Ok(())
}
