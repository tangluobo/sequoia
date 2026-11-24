use super::*;
use chrono::NaiveDate;
use tomato_core::game_dungeons::Weekday;

fn day(value: &str) -> i32 {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .unwrap()
        .num_days_from_ce()
}
fn apply(snapshot: Snapshot, path: &std::path::Path, action: Action, today: i32) -> Snapshot {
    execute_on(path.to_owned(), action, snapshot.revision, today).unwrap()
}
fn fixture(path: &std::path::Path) -> Snapshot {
    let today = day("2026-11-19"); // Thursday
    let mut snapshot = execute_on(path.to_owned(), Action::Load, None, today).unwrap();
    snapshot = apply(
        snapshot,
        path,
        Action::SaveAccount {
            id: None,
            name: "  主账号  ".into(),
        },
        today,
    );
    let account_id = snapshot.board.accounts[0].id.clone();
    for name in ["剑客", "法师"] {
        snapshot = apply(
            snapshot,
            path,
            Action::SaveCharacter {
                id: None,
                account_id: account_id.clone(),
                name: name.into(),
            },
            today,
        );
    }
    for (name, reset_day) in [
        ("周四团本", Weekday::Thursday),
        ("周一秘境", Weekday::Monday),
    ] {
        snapshot = apply(
            snapshot,
            path,
            Action::SaveDungeon {
                id: None,
                name: name.into(),
                reset_day,
            },
            today,
        );
    }
    snapshot
}
#[test]
fn independent_weekly_resets_survive_restart_offline_weeks_and_year_boundary() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("game-dungeons.json");
    let mut snapshot = fixture(&path);
    let character = snapshot.board.characters[0].id.clone();
    for dungeon in snapshot.board.dungeons.clone() {
        snapshot = apply(
            snapshot,
            &path,
            Action::SetCompleted {
                character_id: character.clone(),
                dungeon_id: dungeon.id,
                completed: true,
                period_start: dungeon.reset_day.period_start(day("2026-11-19")),
            },
            day("2026-11-19"),
        );
    }
    let snapshot = execute_on(path.clone(), Action::Load, None, day("2026-11-23"))?;
    assert_eq!(snapshot.board.accounts[0].name, "主账号");
    let board = &snapshot.board;
    assert!(board.completed(&character, &board.dungeons[0], day("2026-11-25")));
    assert!(!board.completed(&character, &board.dungeons[0], day("2026-11-26")));
    assert!(board.completed(&character, &board.dungeons[1], day("2026-11-22")));
    assert!(!board.completed(&character, &board.dungeons[1], day("2026-11-23")));
    assert!(!board.completed(
        &board.characters[1].id,
        &board.dungeons[0],
        day("2026-11-19")
    ));
    assert!(!board.completed(&character, &board.dungeons[0], day("2027-01-07")));
    for weekday in Weekday::ALL {
        for offset in 0..21 {
            let today = day("2026-12-25") + offset;
            let start = weekday.period_start(today);
            assert!((0..7).contains(&(today - start)));
            assert_eq!(
                NaiveDate::from_num_days_from_ce_opt(start)
                    .unwrap()
                    .weekday()
                    .num_days_from_monday(),
                weekday as u32
            );
            assert_eq!(weekday.period_start(today + 7), start + 7);
        }
    }
    Ok(())
}
#[test]
fn unmark_rename_move_reset_day_change_and_cascade_delete_keep_references_valid() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("board.json");
    let today = day("2026-11-19");
    let mut snapshot = fixture(&path);
    let character_id = snapshot.board.characters[0].id.clone();
    let dungeon_id = snapshot.board.dungeons[0].id.clone();
    let mark = |completed| Action::SetCompleted {
        character_id: character_id.clone(),
        dungeon_id: dungeon_id.clone(),
        completed,
        period_start: Weekday::Thursday.period_start(today),
    };
    snapshot = apply(snapshot, &path, mark(true), today);
    snapshot = apply(snapshot, &path, mark(false), today);
    assert!(snapshot.board.completions.is_empty());
    snapshot = apply(snapshot, &path, mark(true), today);
    snapshot = apply(
        snapshot,
        &path,
        Action::SaveDungeon {
            id: Some(dungeon_id.clone()),
            name: "新团本名称".into(),
            reset_day: Weekday::Thursday,
        },
        today,
    );
    assert!(
        snapshot
            .board
            .completed(&character_id, &snapshot.board.dungeons[0], today)
    );
    snapshot = apply(
        snapshot,
        &path,
        Action::SaveAccount {
            id: None,
            name: "小号".into(),
        },
        today,
    );
    let account_id = snapshot.board.accounts[1].id.clone();
    snapshot = apply(
        snapshot,
        &path,
        Action::SaveCharacter {
            id: Some(character_id.clone()),
            account_id: account_id.clone(),
            name: "剑客转服".into(),
        },
        today,
    );
    assert!(
        snapshot
            .board
            .completed(&character_id, &snapshot.board.dungeons[0], today)
    );
    snapshot = apply(
        snapshot,
        &path,
        Action::SaveDungeon {
            id: Some(dungeon_id),
            name: "新团本名称".into(),
            reset_day: Weekday::Monday,
        },
        today,
    );
    assert!(snapshot.board.completions.is_empty());
    snapshot = apply(
        snapshot,
        &path,
        Action::DeleteAccount { id: account_id },
        today,
    );
    assert_eq!(snapshot.board.characters.len(), 1);
    let id = snapshot.board.characters[0].id.clone();
    snapshot = apply(snapshot, &path, Action::DeleteCharacter { id }, today);
    assert!(snapshot.board.characters.is_empty());
    let id = snapshot.board.dungeons[0].id.clone();
    snapshot = apply(snapshot, &path, Action::DeleteDungeon { id }, today);
    assert_eq!(snapshot.board.dungeons.len(), 1);
    snapshot.board.validate()?;
    Ok(())
}
#[test]
fn stale_revisions_corruption_invalid_actions_and_midnight_races_do_not_overwrite_data()
-> Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("board.json");
    let snapshot = fixture(&path);
    let before = std::fs::read(&path)?;
    for action in [
        Action::SaveAccount {
            id: None,
            name: "主账号".into(),
        },
        Action::SaveAccount {
            id: None,
            name: " ".into(),
        },
        Action::SaveAccount {
            id: Some("missing".into()),
            name: "消失".into(),
        },
        Action::SaveCharacter {
            id: None,
            account_id: "missing".into(),
            name: "孤儿".into(),
        },
        Action::SetCompleted {
            character_id: snapshot.board.characters[0].id.clone(),
            dungeon_id: snapshot.board.dungeons[0].id.clone(),
            completed: true,
            period_start: day("2026-11-19"),
        },
    ] {
        assert!(
            execute_on(
                path.clone(),
                action,
                snapshot.revision.clone(),
                day("2026-11-26")
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path)?, before);
    }
    std::fs::write(&path, [before.as_slice(), b"\n"].concat())?;
    let external = std::fs::read(&path)?;
    assert!(
        execute_on(
            path.clone(),
            Action::DeleteAccount {
                id: snapshot.board.accounts[0].id.clone()
            },
            snapshot.revision,
            day("2026-11-19")
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path)?, external);
    std::fs::write(&path, b"broken json")?;
    assert!(execute_on(path.clone(), Action::Load, None, day("2026-11-19")).is_err());
    assert!(
        execute_on(
            path.clone(),
            Action::SaveAccount {
                id: None,
                name: "A".into()
            },
            None,
            day("2026-11-19")
        )
        .is_err()
    );
    assert_eq!(std::fs::read(path)?, b"broken json");
    Ok(())
}
#[tokio::test]
async fn shared_command_facade_preserves_unknown_fields_and_backs_up_original() -> Result<()> {
    use crate::commands::{Command, CommandHandler, Engine, Output};
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("board.json");
    let mut value = serde_json::to_value(Board::default())?;
    value["future_setting"] = serde_json::json!({"theme":"original"});
    let original = serde_json::to_vec(&value)?;
    std::fs::write(&path, &original)?;
    let mut engine = Engine::default();
    let Output::GameDungeons(snapshot) = engine
        .execute(Command::GameDungeons {
            path: path.clone(),
            action: Action::Load,
            revision: None,
        })
        .await?
    else {
        panic!("snapshot")
    };
    let Output::GameDungeons(snapshot) = engine
        .execute(Command::GameDungeons {
            path: path.clone(),
            action: Action::SaveAccount {
                id: None,
                name: "Test".into(),
            },
            revision: snapshot.revision,
        })
        .await?
    else {
        panic!("snapshot")
    };
    assert_eq!(snapshot.board.extra["future_setting"]["theme"], "original");
    assert_eq!(
        std::fs::read(directory.path().join("board.json.before-rust.bak"))?,
        original
    );
    assert!(!directory.path().join("connections.json").exists());
    Ok(())
}
