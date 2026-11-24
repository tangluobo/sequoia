//! File use cases shared by the GUI task queue and CLI command handler.
mod repository;
#[cfg(test)]
mod tests;
use anyhow::Result;
use chrono::{Datelike, Local};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub use tomato_core::game_dungeons::Action;
use tomato_core::game_dungeons::Board;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub board: Board,
    pub revision: Option<String>,
}

pub fn execute(path: PathBuf, action: Action, revision: Option<String>) -> Result<Snapshot> {
    execute_on(
        path,
        action,
        revision,
        Local::now().date_naive().num_days_from_ce(),
    )
}

fn execute_on(
    path: PathBuf,
    action: Action,
    revision: Option<String>,
    today: i32,
) -> Result<Snapshot> {
    let repository = repository::FileRepository::new(path);
    let mut snapshot = repository.load()?;
    if !matches!(action, Action::Load) {
        anyhow::ensure!(
            snapshot.revision == revision,
            "副本记录已被其他窗口或程序修改，请点击刷新后重试；本次未覆盖外部修改"
        );
        snapshot.board.apply(action, today)?;
        snapshot.revision = Some(repository.save(&snapshot.board, revision.as_deref())?);
    }
    Ok(snapshot)
}
