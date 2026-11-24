use std::collections::{BTreeMap, BTreeSet};
use tomato_services::redis_data::{Action, Output};

#[derive(Default)]
pub struct Node {
    pub key: String,
    pub label: String,
    pub value: bool,
    pub children: BTreeMap<String, Node>,
}
#[derive(Default)]
pub struct State {
    pub database: u32,
    pub editor: super::text_state::Editor,
    pub loaded: bool,
    pub pattern: String,
    pub cursor: u64,
    pub keys: BTreeSet<String>,
    pub root: Node,
    pub expanded: BTreeSet<String>,
    pub anchor: Option<String>,
    pub focused: Option<String>,
    pub scroll_to_focus: bool,
    pub selected: BTreeSet<String>,
    pub current: Option<String>,
    pub kind: String,
    pub text: String,
    pub ttl: i64,
    pub received: f64,
    pub dialog: Option<Dialog>,
    pub dialog_focus: bool,
    pub dialog_editors: [super::text_state::Editor; 2],
    pub status: String,
    pub followup: Option<Action>,
    pub scan_pages: usize,
    pub scan_pattern: Option<String>,
    pub failure: Option<Failure>,
}
pub struct Failure {
    pub title: &'static str,
    pub text: String,
    pub focus: bool,
}
pub enum Dialog {
    Add {
        key: String,
        kind: String,
        text: String,
    },
    Rename {
        key: String,
        name: String,
    },
    Ttl {
        key: String,
        seconds: String,
    },
    Delete {
        keys: Vec<String>,
    },
}
impl State {
    pub fn begin_scan(&mut self) -> Action {
        self.keys = BTreeSet::new();
        self.root = Node::default();
        self.selected.clear();
        self.anchor = None;
        self.focused = None;
        self.scroll_to_focus = false;
        self.cursor = 0;
        self.scan_pages = 0;
        self.scan_pattern = Some(self.pattern.clone());
        self.followup = None;
        self.loaded = true;
        Action::Scan {
            pattern: self.pattern.clone(),
            cursor: 0,
        }
    }
    pub fn single_selected(&self) -> Option<&str> {
        (self.selected.len() == 1).then(|| self.selected.first().unwrap().as_str())
    }
    pub fn open_rename(&mut self) {
        if let Some(key) = self.single_selected() {
            self.dialog = Some(Dialog::Rename {
                key: key.into(),
                name: key.into(),
            });
            self.reset_dialog();
        }
    }
    pub fn open_ttl(&mut self) {
        if let Some(key) = self.single_selected() {
            self.dialog = Some(Dialog::Ttl {
                key: key.into(),
                seconds: "3600".into(),
            });
            self.reset_dialog();
        }
    }
    pub fn open_delete(&mut self) {
        if !self.selected.is_empty() {
            self.dialog = Some(Dialog::Delete {
                keys: self.selected.iter().cloned().collect(),
            });
            self.reset_dialog();
        }
    }
    pub fn open_add(&mut self) {
        self.dialog = Some(Dialog::Add {
            key: String::new(),
            kind: "string".into(),
            text: String::new(),
        });
        self.reset_dialog();
    }
    fn reset_dialog(&mut self) {
        self.dialog_focus = true;
        for editor in &mut self.dialog_editors {
            editor.reset();
        }
    }
    pub fn release(&self, ctx: &egui::Context) {
        self.editor.release(ctx);
        for editor in &self.dialog_editors {
            editor.release(ctx);
        }
    }
    pub fn fail(&mut self, text: String) {
        self.followup = None;
        self.loaded = true;
        self.failure = Some(Failure {
            title: "操作失败",
            text,
            focus: true,
        });
    }
    pub fn accept(&mut self, output: Output, now: f64) {
        match output {
            Output::Databases(_) => {}
            Output::Keys { keys, cursor } => {
                self.loaded = true;
                let pattern = self
                    .scan_pattern
                    .get_or_insert_with(|| self.pattern.clone());
                self.cursor = cursor;
                self.scan_pages += 1;
                let mut limited = false;
                let mut bytes: usize = self.keys.iter().map(String::len).sum();
                for key in keys {
                    if self.keys.contains(&key) {
                        continue;
                    }
                    if self.keys.len() >= 10_000 || bytes.saturating_add(key.len()) > 1024 * 1024 {
                        limited = true;
                        self.cursor = 0;
                        self.status = "Key 目录已达内存上限，请缩小搜索范围".into();
                        break;
                    }
                    bytes += key.len();
                    self.keys.insert(key);
                }
                self.root = Node::default();
                for key in &self.keys {
                    let mut node = &mut self.root;
                    let mut path = String::new();
                    // Pathological keys must not create unbounded recursive view state.
                    let parts: Vec<_> = if key.len() > 4096 {
                        vec![key.as_str()]
                    } else {
                        key.split(':').take(33).collect()
                    };
                    let parts = if parts.len() > 32 || key.len() > 4096 {
                        vec![key.as_str()]
                    } else {
                        parts
                    };
                    for (index, part) in parts.into_iter().enumerate() {
                        if index > 0 {
                            path.push(':');
                        }
                        path.push_str(part);
                        node = node.children.entry(part.into()).or_insert_with(|| Node {
                            key: path.clone(),
                            label: part.into(),
                            ..Default::default()
                        });
                    }
                    node.value = true;
                }
                if !limited {
                    self.status.clear();
                }
                if self.cursor != 0 {
                    if self.scan_pages < 4096 {
                        self.followup = Some(Action::Scan {
                            pattern: pattern.clone(),
                            cursor: self.cursor,
                        });
                    } else {
                        self.cursor = 0;
                        self.status = "Key 扫描已达批次上限，请缩小搜索范围".into();
                    }
                } else {
                    self.expanded.retain(|key| contains(&self.root, key));
                    self.selected.retain(|key| contains(&self.root, key));
                }
            }
            Output::Value {
                key,
                kind,
                text,
                ttl,
            } => {
                self.editor.reset();
                self.current = Some(key);
                self.kind = kind;
                self.text = text;
                self.ttl = ttl;
                self.received = now;
            }
            Output::Changed { key, message } => {
                self.dialog = None;
                self.status = message;
                if let Some(key) = key {
                    self.current = Some(key.clone());
                    self.selected = BTreeSet::from([key.clone()]);
                    self.focused = Some(key.clone());
                    self.scroll_to_focus = true;
                    for (byte, _) in key.match_indices(':').take(32) {
                        self.expanded.insert(key[..byte].into());
                    }
                    self.followup = Some(Action::Read { key });
                } else {
                    self.current = None;
                    self.text = String::new();
                    self.editor.reset();
                    self.selected.clear();
                    self.focused = None;
                    self.anchor = None;
                    self.scroll_to_focus = false;
                }
                self.keys.clear();
                self.root = Node::default();
                self.cursor = 0;
                self.scan_pages = 0;
                self.loaded = false;
            }
            Output::DeleteReport {
                succeeded,
                failed,
                failures,
            } => {
                self.accept(
                    Output::Changed {
                        key: None,
                        message: "Key 列表已刷新".into(),
                    },
                    now,
                );
                let mut text = format!("已完成 {succeeded} 个，失败或未完成 {failed} 个。\n");
                for (key, error) in &failures {
                    text.push_str(&format!("\n{key}: {error}"));
                }
                if failed > failures.len() {
                    text.push_str(&format!("\n另有 {} 项未列出。", failed - failures.len()));
                }
                self.failure = Some(Failure {
                    title: if succeeded == 0 {
                        "删除失败"
                    } else {
                        "部分删除失败"
                    },
                    text,
                    focus: true,
                });
            }
        }
    }
}

fn contains(root: &Node, path: &str) -> bool {
    if root.children.contains_key(path) {
        return true;
    }
    let mut node = root;
    for part in path.split(':') {
        let Some(child) = node.children.get(part) else {
            return false;
        };
        node = child;
    }
    true
}

pub fn ttl_label(ttl: i64) -> (String, egui::Color32) {
    let text = match ttl {
        -1 => "永不过期".into(),
        -2 => "Key不存在".into(),
        i64::MIN..=0 => "已过期".into(),
        1..=59 => format!("{ttl}秒"),
        60..=3599 => format!("{}分{}秒", ttl / 60, ttl % 60),
        3600..=86399 => format!("{}时{}分{}秒", ttl / 3600, ttl % 3600 / 60, ttl % 60),
        _ => format!(
            "{}天{}时{}分",
            ttl / 86400,
            ttl % 86400 / 3600,
            ttl % 3600 / 60
        ),
    };
    let color = if ttl != -1 && ttl < 60 {
        egui::Color32::from_rgb(204, 0, 0)
    } else if (60..300).contains(&ttl) {
        egui::Color32::from_rgb(230, 167, 0)
    } else {
        egui::Color32::from_gray(102)
    };
    (format!("TTL: {text}"), color)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scans_continue_across_empty_batches_but_stop_at_memory_and_page_limits() {
        let mut state = State {
            pattern: "first".into(),
            ..Default::default()
        };
        state.begin_scan();
        state.pattern = "unsubmitted edit".into();
        state.accept(
            Output::Keys {
                keys: vec![],
                cursor: 42,
            },
            0.,
        );
        assert!(matches!(
            state.followup.take(),
            Some(Action::Scan { cursor: 42, pattern }) if pattern == "first"
        ));
        state.accept(
            Output::Keys {
                keys: vec!["a:b".into()],
                cursor: 0,
            },
            0.,
        );
        assert!(state.followup.is_none());
        assert!(state.root.children["a"].children["b"].value);
        state.scan_pages = 4095;
        state.accept(
            Output::Keys {
                keys: vec![],
                cursor: 1,
            },
            0.,
        );
        assert_eq!(state.cursor, 0);
        assert!(state.followup.is_none());
        state.begin_scan();
        state.accept(
            Output::Keys {
                keys: vec!["x".repeat(1024 * 1024 + 1)],
                cursor: 2,
            },
            0.,
        );
        assert!(
            state.keys.is_empty() && state.root.children.is_empty() && state.followup.is_none()
        );
        assert_eq!(state.cursor, 0);
        state.fail("denied".into());
        assert!(state.loaded && state.failure.is_some());
    }
    #[test]
    fn empty_prefixes_are_distinct_and_pathological_keys_remain_shallow() {
        let mut state = State::default();
        let deep = "x:".repeat(10000);
        state.accept(
            Output::Keys {
                keys: vec![":a".into(), "a::b".into(), deep.clone()],
                cursor: 0,
            },
            0.,
        );
        assert_eq!(state.root.children[""].children["a"].key, ":a");
        assert_eq!(
            state.root.children["a"].children[""].children["b"].key,
            "a::b"
        );
        assert!(state.root.children[&deep].children.is_empty());
        assert_eq!(ttl_label(3661).0, "TTL: 1时1分1秒");
        assert_eq!(ttl_label(90061).0, "TTL: 1天1时1分");
    }
    #[test]
    fn changed_key_refreshes_value_then_key_tree_without_losing_scan() {
        let mut state = State {
            loaded: true,
            ..Default::default()
        };
        state.accept(
            Output::Changed {
                key: Some("a".into()),
                message: "saved".into(),
            },
            0.,
        );
        assert!(!state.loaded);
        assert!(matches!(state.followup.take(), Some(Action::Read { .. })));
        state.accept(
            Output::Value {
                key: "a".into(),
                kind: "string".into(),
                text: "value".into(),
                ttl: -1,
            },
            1.,
        );
        assert!(
            !state.loaded,
            "reading value must not suppress the subsequent tree refresh"
        );
    }
}
