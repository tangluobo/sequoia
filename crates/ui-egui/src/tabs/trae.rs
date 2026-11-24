use std::{path::PathBuf, sync::Arc};
use tomato_services::trae::{Mutation, Output, Snapshot};
#[derive(Default)]
pub struct State {
    pub path: Option<PathBuf>,
    pub snapshot: Option<Arc<Snapshot>>,
    pub selected: Option<String>,
    pub tasks: Vec<String>,
    pub task_labels: Vec<String>,
    pub task_heights: Vec<f32>,
    pub task_width: f32,
    pub generation: u64,
    pub pending_tasks: bool,
    pub pending_selection: Option<String>,
    pub dialog: Option<Dialog>,
    pub status: String,
    pub task_status: String,
    pub status_until: f64,
}
pub enum Dialog {
    Add {
        phone: String,
        name: String,
    },
    Rename {
        phone: String,
        name: String,
    },
    Delete {
        phone: String,
        active: bool,
        step: usize,
    },
}
impl State {
    pub fn select(&mut self, phone: String) {
        self.selected = Some(phone);
        self.generation = self.generation.wrapping_add(1);
        self.tasks.clear();
        self.task_labels.clear();
        self.task_heights.clear();
        self.pending_tasks = true;
        self.task_status = "加载中...".into();
    }
    pub fn accept(&mut self, output: Output, now: f64) {
        match output {
            Output::Snapshot(snapshot) => {
                self.status = if self.snapshot.is_none() && snapshot.warning.is_empty() {
                    String::new()
                } else if snapshot.warning.is_empty() {
                    "操作完成".into()
                } else {
                    snapshot.warning.clone()
                };
                self.status_until = now + 4.;
                let selected = self
                    .pending_selection
                    .take()
                    .or_else(|| self.selected.clone())
                    .filter(|phone| snapshot.config.sessions.iter().any(|s| &s.phone == phone))
                    .or_else(|| snapshot.config.sessions.first().map(|s| s.phone.clone()));
                self.snapshot = Some(Arc::new(snapshot));
                self.dialog = None;
                if let Some(phone) = selected {
                    self.select(phone);
                } else {
                    self.selected = None;
                    self.tasks.clear();
                    self.task_labels.clear();
                    self.pending_tasks = false;
                    self.task_status = "请选择会话以查看历史任务".into();
                }
            }
            Output::Tasks {
                phone,
                generation,
                tasks,
                status,
                truncated,
            } if self.selected.as_deref() == Some(&phone) && generation == self.generation => {
                self.task_labels = tasks
                    .iter()
                    .map(|text| {
                        let mut display: String = text.chars().take(4096).collect();
                        if display.len() < text.len() {
                            display.push('…');
                        }
                        display
                    })
                    .collect();
                self.tasks = tasks;
                self.task_heights = vec![54.; self.tasks.len()];
                self.task_width = 0.;
                self.task_status = status;
                if truncated {
                    self.task_status
                        .push_str(" 历史展示已达 8 MiB/10000 条上限");
                }
            }
            _ => {}
        }
    }
    pub fn reorder(&self, from: &str, to: Option<(&str, bool)>) -> Option<Mutation> {
        let snapshot = self.snapshot.as_ref()?;
        let mut phones: Vec<_> = snapshot
            .config
            .sessions
            .iter()
            .map(|s| s.phone.clone())
            .collect();
        let index = phones.iter().position(|p| p == from)?;
        let phone = phones.remove(index);
        let target = if let Some((target, after)) = to {
            phones.iter().position(|p| p == target)? + usize::from(after)
        } else {
            phones.len()
        };
        phones.insert(target, phone);
        Some(Mutation::Reorder { phones })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tomato_services::trae::{Config, Session};
    #[test]
    fn late_tasks_cannot_replace_new_selection_and_reorder_keeps_identity() {
        let mut state = State::default();
        state.accept(
            Output::Snapshot(Snapshot {
                config: Config {
                    sessions: vec![
                        Session {
                            phone: "1".into(),
                            ..Default::default()
                        },
                        Session {
                            phone: "2".into(),
                            ..Default::default()
                        },
                        Session {
                            phone: "3".into(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                },
                stamp: None,
                user_exists: false,
                exists: vec![true; 3],
                warning: String::new(),
            }),
            0.,
        );
        let old = state.generation;
        state.select("2".into());
        state.accept(
            Output::Tasks {
                phone: "1".into(),
                generation: old,
                tasks: vec!["stale".into()],
                status: String::new(),
                truncated: false,
            },
            1.,
        );
        assert!(state.tasks.is_empty());
        state.accept(
            Output::Tasks {
                phone: "2".into(),
                generation: state.generation,
                tasks: vec!["current".into()],
                status: String::new(),
                truncated: false,
            },
            1.,
        );
        assert_eq!(state.tasks, ["current"]);
        assert!(
            matches!(state.reorder("1",Some(("3",true))),Some(Mutation::Reorder {phones}) if phones==["2","3","1"])
        );
        assert!(
            matches!(state.reorder("3",Some(("1",false))),Some(Mutation::Reorder {phones}) if phones==["3","1","2"])
        );
    }
}
