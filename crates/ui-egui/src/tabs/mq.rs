use serde_json::Value;
use tomato_services::mq::{Action, Output};
mod body;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Topics,
    Messages,
    Groups,
    Group,
    Cluster,
    Broker,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum QueryPage {
    Time,
    Offset,
    Key,
    Id,
    Send,
    Unconsumed,
    ConsumeStatus,
}
pub struct State {
    pub send_body_editor: super::text_state::Editor,
    pub headers_editor: super::text_state::Editor,
    pub send_key: String,
    pub begin_picker: Option<super::query_results::temporal::Picker>,
    pub end_picker: Option<super::query_results::temporal::Picker>,
    pub body_view: super::json::JsonView,
    pub body_reset: bool,
    pub consume_rows: Vec<Value>,
    pub consume_selected: Option<usize>,
    pub control: Option<tomato_services::backup::Control>,
    pub page: Page,
    pub query: QueryPage,
    pub topic: String,
    pub group: String,
    pub address: String,
    pub rows: Vec<Value>,
    pub selected: Option<usize>,
    pub group_selection: crate::row_selection::Selection,
    pub batch_notice: Option<(String, String)>,
    pub detail: String,
    pub body: String,
    pub body_container: bool,
    pub loaded: bool,
    pub pending: Option<Action>,
    pub followup: Option<Action>,
    pub status: String,
    pub partition: String,
    pub send_partition: String,
    pub offset: String,
    pub limit: usize,
    pub begin: String,
    pub end: String,
    pub key: String,
    pub id: String,
    pub tags: String,
    pub delay: i32,
    pub send_body: String,
    pub headers: String,
    pub create: bool,
    pub new_topic: String,
    pub partitions: i32,
    pub replicas: i16,
    pub confirm: Option<Action>,
    pub detail_dialog: bool,
}
impl Default for State {
    fn default() -> Self {
        let now = chrono::Local::now();
        Self {
            send_body_editor: Default::default(),
            headers_editor: Default::default(),
            send_key: String::new(),
            begin_picker: None,
            end_picker: None,
            body_view: Default::default(),
            body_reset: true,
            consume_rows: Vec::new(),
            consume_selected: None,
            control: None,
            page: Page::Topics,
            query: QueryPage::Time,
            topic: String::new(),
            group: String::new(),
            address: String::new(),
            rows: Vec::new(),
            selected: None,
            group_selection: Default::default(),
            batch_notice: None,
            detail: String::new(),
            body: String::new(),
            body_container: false,
            loaded: false,
            pending: None,
            followup: None,
            status: String::new(),
            partition: "0".into(),
            send_partition: String::new(),
            offset: "0".into(),
            limit: 100,
            begin: (now.date_naive() - chrono::Duration::days(3))
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            end: now
                .date_naive()
                .and_hms_opt(23, 59, 59)
                .unwrap()
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
            key: String::new(),
            id: String::new(),
            tags: String::new(),
            delay: 0,
            send_body: String::new(),
            headers: String::new(),
            create: false,
            new_topic: String::new(),
            partitions: 4,
            replicas: 1,
            confirm: None,
            detail_dialog: false,
        }
    }
}
impl State {
    pub fn action(&self) -> Option<Action> {
        match self.page {
            Page::Topics => Some(if self.topic.is_empty() {
                Action::Topics
            } else {
                Action::Stats {
                    topic: self.topic.clone(),
                }
            }),
            Page::Groups => Some(Action::Groups),
            Page::Group => Some(Action::Group {
                group: self.group.clone(),
            }),
            Page::Cluster => Some(Action::Cluster),
            Page::Broker => Some(Action::Broker {
                address: self.address.clone(),
            }),
            Page::Messages => None,
        }
    }
    pub fn release(&self, ctx: &egui::Context) {
        self.send_body_editor.release(ctx);
        self.headers_editor.release(ctx);
        for picker in [&self.begin_picker, &self.end_picker].into_iter().flatten() {
            picker.release(ctx);
        }
    }
    pub fn accept(&mut self, output: Output) {
        self.status = output.message;
        let action = self.pending.take();
        if matches!(action, Some(Action::ConsumeStatus { .. })) {
            self.consume_rows = output.rows;
            self.consume_selected = None;
            return;
        }
        if matches!(action, Some(Action::Track { .. })) {
            self.detail = serde_json::to_string_pretty(&output.rows).unwrap_or_default();
            self.detail_dialog = true;
            return;
        }
        let change = matches!(
            action,
            Some(
                Action::CreateTopic { .. }
                    | Action::DeleteTopic { .. }
                    | Action::DeleteGroup { .. }
                    | Action::DeleteGroups { .. }
            )
        );
        if change {
            if matches!(action, Some(Action::DeleteGroups { .. })) {
                let errors = output.detail["failed"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|v| {
                        format!(
                            "{}: {}",
                            v["group"].as_str().unwrap_or_default(),
                            v["error"].as_str().unwrap_or_default()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                self.batch_notice = Some((
                    if errors.is_empty() {
                        "成功"
                    } else {
                        "部分删除失败"
                    }
                    .into(),
                    if errors.is_empty() {
                        self.status.clone()
                    } else {
                        format!("{}\n\n{errors}", self.status)
                    },
                ));
            }
            if matches!(
                action,
                Some(
                    Action::DeleteTopic { .. }
                        | Action::DeleteGroup { .. }
                        | Action::DeleteGroups { .. }
                )
            ) {
                self.rows.clear();
                self.selected = None;
                self.group_selection.clear();
                self.loaded = false;
            }
            self.create = false;
            self.confirm = None;
            if matches!(action, Some(Action::DeleteTopic { .. })) {
                self.topic.clear();
                self.page = Page::Topics;
            }
            if matches!(
                action,
                Some(Action::DeleteGroup { .. } | Action::DeleteGroups { .. })
            ) {
                self.group.clear();
                self.page = Page::Groups;
            }
            self.followup = self.action();
        } else {
            self.rows = if self.page == Page::Broker && output.rows.is_empty() {
                output
                    .detail
                    .get("table")
                    .and_then(Value::as_object)
                    .into_iter()
                    .flatten()
                    .map(|(k, v)| serde_json::json!({"name":k,"value":v}))
                    .collect()
            } else {
                output.rows
            };
            self.selected = None;
            self.group_selection.clear();
            self.set_body(String::new());
            self.detail = if output.detail.is_null() {
                String::new()
            } else {
                serde_json::to_string_pretty(&output.detail).unwrap_or_default()
            };
            self.loaded = true;
        }
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some(c) = &self.control {
            c.cancel();
        }
    }
}
