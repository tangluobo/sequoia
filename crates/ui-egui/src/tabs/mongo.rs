use std::collections::VecDeque;
use tomato_services::mongo::{Action, Output, Target};

pub struct State {
    pub target: Target,
    pub page: usize,
    pub filter: String,
    pub skip: u64,
    pub limit: u32,
    pub documents: Vec<String>,
    pub document_labels: Vec<String>,
    pub selected: Option<usize>,
    pub text: String,
    pub command: String,
    pub result: String,
    pub indexes: Vec<String>,
    pub index_labels: Vec<String>,
    pub selected_index: Option<usize>,
    pub index_keys: String,
    pub index_name: String,
    pub unique: bool,
    pub validator: String,
    pub status: String,
    pub pending: VecDeque<Action>,
    pub confirm: Option<Confirmation>,
    pub editors: [super::text_state::Editor; 4],
}
pub enum Confirmation {
    Document,
    Index(String),
}
impl State {
    pub fn new(target: Target) -> Self {
        let mut state = Self {
            target,
            page: 0,
            filter: "{}".into(),
            skip: 0,
            limit: 100,
            documents: Vec::new(),
            document_labels: Vec::new(),
            selected: None,
            text: "{}".into(),
            command: "{\n  \"dbStats\": 1\n}".into(),
            result: String::new(),
            indexes: Vec::new(),
            index_labels: Vec::new(),
            selected_index: None,
            index_keys: "{\"field\": 1}".into(),
            index_name: String::new(),
            unique: false,
            validator: "{}".into(),
            status: String::new(),
            pending: VecDeque::new(),
            confirm: None,
            editors: Default::default(),
        };
        state.pending.push_back(state.find());
        state.pending.push_back(Action::Indexes {
            target: state.target.clone(),
        });
        state.pending.push_back(Action::Validator {
            target: state.target.clone(),
        });
        state
    }
    pub fn find(&self) -> Action {
        Action::Find {
            target: self.target.clone(),
            filter: self.filter.clone(),
            skip: self.skip,
            limit: self.limit,
        }
    }
    pub fn select(&mut self, index: usize) {
        if let Some(text) = self.documents.get(index) {
            self.text = text.clone();
            self.selected = Some(index);
            self.editors[0].reset();
        }
    }
    pub fn original(&self) -> Option<&str> {
        self.selected
            .and_then(|i| self.documents.get(i))
            .map(String::as_str)
    }
    pub fn accept(&mut self, output: Output) {
        match output {
            Output::Documents {
                target,
                documents,
                truncated,
            } if target == self.target => {
                self.document_labels = documents.iter().map(|s| summary(s)).collect();
                self.documents = documents;
                self.selected = None;
                self.text = "{}".into();
                self.editors[0].reset();
                if !self.documents.is_empty() {
                    self.select(0);
                }
                self.status = format!(
                    "已加载 {} 条文档{}",
                    self.documents.len(),
                    if truncated {
                        "（已达 8 MiB 保留上限）"
                    } else {
                        ""
                    }
                );
            }
            Output::Document { target, .. } if target == self.target => {
                self.confirm = None;
                self.status = "文档已保存".into();
                self.pending.push_front(self.find());
            }
            Output::Indexes { target, indexes } if target == self.target => {
                self.index_labels = indexes.iter().map(|s| summary(s)).collect();
                self.indexes = indexes;
                self.selected_index = None;
            }
            Output::Validator { target, json } if target == self.target => {
                self.validator = json;
                self.editors[3].reset();
            }
            Output::Text(text) => {
                self.result = text;
                self.editors[2].reset();
                self.status = "命令执行成功".into();
            }
            Output::Changed { target, kind } if target == self.target => {
                self.confirm = None;
                self.status = "操作成功".into();
                match kind.as_str() {
                    "documents" | "documents_missing" => self.pending.push_front(self.find()),
                    "indexes" => self.pending.push_front(Action::Indexes {
                        target: self.target.clone(),
                    }),
                    "validator" => self.pending.push_front(Action::Validator {
                        target: self.target.clone(),
                    }),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    pub fn release(&self, ctx: &egui::Context) {
        for editor in &self.editors {
            editor.release(ctx);
        }
    }
}
fn summary(text: &str) -> String {
    let mut summary = String::new();
    let mut space = false;
    for c in text.chars() {
        // Java's default \\s is ASCII, and String.length counts UTF-16 units.
        if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}') {
            space = true;
            continue;
        }
        if space && !summary.is_empty() {
            summary.push(' ');
        }
        space = false;
        summary.push(c);
    }
    if summary.encode_utf16().count() > 240 {
        let mut units = 0;
        let end = summary
            .char_indices()
            .find_map(|(index, c)| {
                units += c.len_utf16();
                (units > 237).then_some(index)
            })
            .unwrap_or(summary.len());
        summary.truncate(end);
        summary.push('…');
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collection_mutations_reload_the_right_view_and_stale_outputs_cannot_replace_editor() {
        let target = Target {
            database: "db".into(),
            collection: "items".into(),
        };
        let mut view = State::new(target.clone());
        view.pending.clear();
        view.accept(Output::Documents {
            target: target.clone(),
            documents: vec!["{\"_id\":1}".into()],
            truncated: false,
        });
        assert_eq!(view.original(), Some("{\"_id\":1}"));
        view.text = "unsaved draft".into();
        view.accept(Output::Documents {
            target: Target {
                collection: "other".into(),
                ..target.clone()
            },
            documents: vec![],
            truncated: false,
        });
        assert_eq!(view.text, "unsaved draft");
        view.accept(Output::Changed {
            target: target.clone(),
            kind: "indexes".into(),
        });
        assert!(matches!(
            view.pending.pop_front(),
            Some(Action::Indexes { .. })
        ));
        assert_eq!(view.text, "unsaved draft");
        view.accept(Output::Document {
            target,
            json: "{}".into(),
        });
        assert!(matches!(
            view.pending.pop_front(),
            Some(Action::Find { .. })
        ));
    }
}
