//! Weekly dungeon checklist, independent of clocks, storage and UI.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const FILE_NAME: &str = "game-dungeons.json";
pub const MAX_ACCOUNTS: usize = 500;
pub const MAX_CHARACTERS: usize = 1000;
pub const MAX_DUNGEONS: usize = 100;
pub type Extra = BTreeMap<String, serde_json::Value>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    #[default]
    Thursday,
    Friday,
    Saturday,
    Sunday,
}
impl Weekday {
    pub const ALL: [Self; 7] = [
        Self::Monday,
        Self::Tuesday,
        Self::Wednesday,
        Self::Thursday,
        Self::Friday,
        Self::Saturday,
        Self::Sunday,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Monday => "周一",
            Self::Tuesday => "周二",
            Self::Wednesday => "周三",
            Self::Thursday => "周四",
            Self::Friday => "周五",
            Self::Saturday => "周六",
            Self::Sunday => "周日",
        }
    }
    /// Dates are proleptic Gregorian days from CE (0001-01-01 = Monday = 1).
    pub fn period_start(self, today: i32) -> i32 {
        today - (today - 1 - self as i32).rem_euclid(7)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub name: String,
    #[serde(default, flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Character {
    pub id: String,
    pub account_id: String,
    pub name: String,
    #[serde(default, flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Dungeon {
    pub id: String,
    pub name: String,
    pub reset_day: Weekday,
    #[serde(default, flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Completion {
    pub day: i32,
    #[serde(default, flatten)]
    pub extra: Extra,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Board {
    pub version: u32,
    pub accounts: Vec<Account>,
    pub characters: Vec<Character>,
    pub dungeons: Vec<Dungeon>,
    /// One record per character/dungeon, bounded independently of elapsed weeks.
    pub completions: BTreeMap<String, BTreeMap<String, Completion>>,
    #[serde(default, flatten)]
    pub extra: Extra,
}
impl Default for Board {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: Vec::new(),
            characters: Vec::new(),
            dungeons: Vec::new(),
            completions: BTreeMap::new(),
            extra: Extra::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    Load,
    SaveAccount {
        id: Option<String>,
        name: String,
    },
    SaveCharacter {
        id: Option<String>,
        account_id: String,
        name: String,
    },
    SaveDungeon {
        id: Option<String>,
        name: String,
        reset_day: Weekday,
    },
    DeleteAccount {
        id: String,
    },
    DeleteCharacter {
        id: String,
    },
    DeleteDungeon {
        id: String,
    },
    SetCompleted {
        character_id: String,
        dungeon_id: String,
        completed: bool,
        period_start: i32,
    },
}

fn name(value: &str) -> Result<String> {
    let value = value.trim();
    ensure!(!value.is_empty(), "名称不能为空");
    ensure!(
        value.chars().count() <= 120 && !value.chars().any(char::is_control),
        "名称最多 120 个字符，且不能包含控制字符"
    );
    Ok(value.to_owned())
}
fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Result<BTreeSet<&'a str>> {
    let mut found = BTreeSet::new();
    for id in ids {
        ensure!(
            !id.is_empty() && id.len() <= 128 && found.insert(id),
            "记录 ID 无效或重复"
        );
    }
    Ok(found)
}
impl Board {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == 1, "不支持的副本管理数据版本，原文件未修改");
        ensure!(
            self.accounts.len() <= MAX_ACCOUNTS
                && self.characters.len() <= MAX_CHARACTERS
                && self.dungeons.len() <= MAX_DUNGEONS,
            "最多支持 500 个账号、1000 个角色和 100 个副本"
        );
        let accounts = unique_ids(self.accounts.iter().map(|a| a.id.as_str()))?;
        let characters = unique_ids(self.characters.iter().map(|c| c.id.as_str()))?;
        let dungeons = unique_ids(self.dungeons.iter().map(|d| d.id.as_str()))?;
        for account in &self.accounts {
            name(&account.name)?;
        }
        for character in &self.characters {
            name(&character.name)?;
            ensure!(
                accounts.contains(character.account_id.as_str()),
                "角色所属账号不存在"
            );
        }
        for dungeon in &self.dungeons {
            name(&dungeon.name)?;
        }
        for (character, entries) in &self.completions {
            ensure!(
                characters.contains(character.as_str()),
                "完成记录中的角色不存在"
            );
            for (dungeon, completion) in entries {
                ensure!(
                    dungeons.contains(dungeon.as_str()),
                    "完成记录中的副本不存在"
                );
                ensure!((1..=3_652_059).contains(&completion.day), "完成日期无效");
            }
        }
        Ok(())
    }
    pub fn completed(&self, character: &str, dungeon: &Dungeon, today: i32) -> bool {
        self.completions
            .get(character)
            .and_then(|entries| entries.get(&dungeon.id))
            .is_some_and(|entry| {
                entry.day >= dungeon.reset_day.period_start(today) && entry.day <= today
            })
    }
    /// The caller discards this working copy if an action or validation fails.
    pub fn apply(&mut self, action: Action, today: i32) -> Result<()> {
        ensure!((1..=3_652_059).contains(&today), "本机日期无效");
        match action {
            Action::Load => {}
            Action::SaveAccount { id, name: value } => {
                let value = name(&value)?;
                ensure!(
                    !self
                        .accounts
                        .iter()
                        .any(|a| a.name == value && Some(&a.id) != id.as_ref()),
                    "账号名称已存在"
                );
                if let Some(id) = id {
                    self.accounts
                        .iter_mut()
                        .find(|a| a.id == id)
                        .ok_or_else(|| anyhow::anyhow!("账号不存在"))?
                        .name = value;
                } else {
                    self.accounts.push(Account {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: value,
                        extra: Extra::new(),
                    });
                }
            }
            Action::SaveCharacter {
                id,
                account_id,
                name: value,
            } => {
                let value = name(&value)?;
                ensure!(
                    self.accounts.iter().any(|a| a.id == account_id),
                    "请先添加或选择账号"
                );
                ensure!(
                    !self.characters.iter().any(|c| c.account_id == account_id
                        && c.name == value
                        && Some(&c.id) != id.as_ref()),
                    "该账号下的角色名称已存在"
                );
                if let Some(id) = id {
                    let character = self
                        .characters
                        .iter_mut()
                        .find(|c| c.id == id)
                        .ok_or_else(|| anyhow::anyhow!("角色不存在"))?;
                    character.name = value;
                    character.account_id = account_id;
                } else {
                    self.characters.push(Character {
                        id: uuid::Uuid::new_v4().to_string(),
                        account_id,
                        name: value,
                        extra: Extra::new(),
                    });
                }
            }
            Action::SaveDungeon {
                id,
                name: value,
                reset_day,
            } => {
                let value = name(&value)?;
                ensure!(
                    !self
                        .dungeons
                        .iter()
                        .any(|d| d.name == value && Some(&d.id) != id.as_ref()),
                    "副本名称已存在"
                );
                if let Some(id) = id {
                    let dungeon = self
                        .dungeons
                        .iter_mut()
                        .find(|d| d.id == id)
                        .ok_or_else(|| anyhow::anyhow!("副本不存在"))?;
                    if dungeon.reset_day != reset_day {
                        for entries in self.completions.values_mut() {
                            entries.remove(&id);
                        }
                    }
                    dungeon.name = value;
                    dungeon.reset_day = reset_day;
                } else {
                    self.dungeons.push(Dungeon {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: value,
                        reset_day,
                        extra: Extra::new(),
                    });
                }
            }
            Action::DeleteAccount { id } => {
                ensure!(self.accounts.iter().any(|a| a.id == id), "账号不存在");
                self.accounts.retain(|a| a.id != id);
                self.characters.retain(|c| {
                    if c.account_id == id {
                        self.completions.remove(&c.id);
                        false
                    } else {
                        true
                    }
                });
            }
            Action::DeleteCharacter { id } => {
                ensure!(self.characters.iter().any(|c| c.id == id), "角色不存在");
                self.characters.retain(|c| c.id != id);
                self.completions.remove(&id);
            }
            Action::DeleteDungeon { id } => {
                ensure!(self.dungeons.iter().any(|d| d.id == id), "副本不存在");
                self.dungeons.retain(|d| d.id != id);
                for entries in self.completions.values_mut() {
                    entries.remove(&id);
                }
            }
            Action::SetCompleted {
                character_id,
                dungeon_id,
                completed,
                period_start,
            } => {
                ensure!(
                    self.characters.iter().any(|c| c.id == character_id),
                    "角色不存在"
                );
                let dungeon = self
                    .dungeons
                    .iter()
                    .find(|d| d.id == dungeon_id)
                    .ok_or_else(|| anyhow::anyhow!("副本不存在"))?;
                ensure!(
                    period_start == dungeon.reset_day.period_start(today),
                    "副本已进入新一周，请刷新后重新标记"
                );
                if completed {
                    self.completions
                        .entry(character_id)
                        .or_default()
                        .entry(dungeon_id)
                        .and_modify(|entry| entry.day = today)
                        .or_insert(Completion {
                            day: today,
                            extra: Extra::new(),
                        });
                } else if let Some(entries) = self.completions.get_mut(&character_id) {
                    entries.remove(&dungeon_id);
                }
            }
        }
        self.completions.retain(|_, entries| !entries.is_empty());
        self.validate()
    }
}
