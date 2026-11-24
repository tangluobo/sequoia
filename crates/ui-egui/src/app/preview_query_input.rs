//! Opt-in input sent only to this application's isolated SQLite preview.
use super::*;
use egui::{Context, Event, Id, Key, Modifiers, PointerButton, RawInput};

const READY: &str = "query-input-preview-ready";
const FRAME: &str = "query-input-preview-frame";
const POINT: &str = "query-input-preview-point";

impl TomatoApp {
    pub(super) fn preview_query_input(&mut self, ctx: &Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("查询输入验收必须使用隔离配置"))?;
        anyhow::ensure!(
            profile.file_name().is_some_and(|name| name
                .to_string_lossy()
                .starts_with("tomato-query-input-fixture-"))
                && self.store.connections.is_empty()
                && std::env::var_os("TOMATO_UI_CAPTURE").is_some(),
            "查询输入验收必须使用独立空配置"
        );
        let mut connection = Connection::new("SQLITE");
        connection.name = "查询输入验收".into();
        self.store.connections.push(connection.clone());
        self.open_connection(connection);
        self.section = 0;
        self.quit_approved = true;
        ctx.data_mut(|data| data.insert_temp(Id::new("query-input-preview-installed"), true));
        ctx.add_plugin(Input { frame: 0 });
        Ok(())
    }

    pub(super) fn query_input_preview(&mut self, ctx: &Context) {
        if !ctx
            .data(|data| data.get_temp::<bool>(Id::new("query-input-preview-installed")))
            .unwrap_or(false)
        {
            return;
        }
        if !ctx
            .data(|data| data.get_temp::<bool>(Id::new(READY)))
            .unwrap_or(false)
        {
            if !self.pending.is_empty() {
                return;
            }
            let Some(connection) = self
                .store
                .connections
                .first()
                .filter(|connection| self.sql_hosts.contains_key(&connection.id))
                .cloned()
            else {
                return;
            };
            if std::env::var("TOMATO_QUERY_INPUT_ROUTE").as_deref() == Ok("tree") {
                self.query_action(query_tree::Action::New {
                    connection: connection.id.clone(),
                    namespace: "main".into(),
                    path: String::new(),
                });
            } else {
                self.new_scoped_query(connection, "main".into());
            }
            ctx.data_mut(|data| data.insert_temp(Id::new(READY), true));
            ctx.request_repaint();
            return;
        }
        let Some(tab) = self
            .tabs
            .iter()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
        else {
            return;
        };
        let Panel::Sql {
            sql,
            workbench,
            results,
            ..
        } = &tab.panel
        else {
            return;
        };
        ctx.data_mut(|data| {
            data.insert_temp(
                Id::new(POINT),
                workbench.completion.galley_pos + egui::vec2(250., 60.),
            )
        });
        let frame = ctx
            .data(|data| data.get_temp::<usize>(Id::new(FRAME)))
            .unwrap_or(0);
        if ![2, 6, 10, 14, 24, 38, 48].contains(&frame) && !(frame > 48 && self.pending.is_empty())
        {
            return;
        }
        let focused = ctx.memory(|memory| memory.focused());
        let proof = serde_json::json!({
            "frame":frame,"sql":sql,"focused":focused.is_some(),
            "focusRequested":workbench.completion.focus_requested,
            "dirty":workbench.document.as_ref().is_some_and(|document| document.dirty()),
            "caret":workbench.completion.caret,
            "lineHeight":workbench.completion.galley.as_ref().and_then(|galley| galley.rows.first()).map(|row|row.size.y),
            "candidates":workbench.completion.matches.candidates.iter().map(|candidate| &candidate.text).collect::<Vec<_>>(),
            "value":results.first().and_then(|result| result.value(0,0)),
            "pending":self.pending.len(),"error":tab.error,
            "osInputUsed":false,"savedHostsContacted":false,"systemClipboardWritten":false
        });
        ctx.data_mut(|data| {
            let samples = data.get_temp_mut_or_default::<Vec<serde_json::Value>>(Id::new(
                "query-input-preview-samples",
            ));
            if samples
                .last()
                .is_some_and(|sample| sample["frame"].as_u64().unwrap_or(0) >= 48)
            {
                samples.pop();
            }
            samples.push(proof);
        });
        if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
            let samples = ctx
                .data(|data| {
                    data.get_temp::<Vec<serde_json::Value>>(Id::new("query-input-preview-samples"))
                })
                .unwrap_or_default();
            let _ = std::fs::write(path.with_extension("input.json"), serde_json::json!({"samples":samples,"ready":frame>=48 && self.pending.is_empty(),"route":std::env::var("TOMATO_QUERY_INPUT_ROUTE").unwrap_or_else(|_|"scoped".into())}).to_string());
        }
    }
}

struct Input {
    frame: usize,
}
fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "isolated new query input"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id != egui::ViewportId::ROOT {
            return;
        }
        input
            .events
            .retain(|event| matches!(event, Event::Screenshot { .. }));
        input.focused = true;
        if !ctx
            .data(|data| data.get_temp::<bool>(Id::new(READY)))
            .unwrap_or(false)
        {
            return;
        }
        self.frame += 1;
        ctx.data_mut(|data| data.insert_temp(Id::new(FRAME), self.frame));
        let mut modifiers = Modifiers::NONE;
        let control = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let event = match self.frame {
            4 => Some(Event::Text("SEL".into())),
            8 => Some(key(Key::Tab, modifiers)),
            12 => Some(Event::Text(" '中文' AS value;".into())),
            18 | 19 => ctx
                .data(|data| data.get_temp::<egui::Pos2>(Id::new(POINT)))
                .map(|pos| {
                    input.events.push(Event::PointerMoved(pos));
                    Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: self.frame == 18,
                        modifiers,
                    }
                }),
            22 => Some(Event::Text(" -- 点击空白".into())),
            26 | 44 => {
                modifiers = control;
                Some(key(Key::Enter, modifiers))
            }
            35 => {
                modifiers = control;
                Some(key(Key::A, modifiers))
            }
            36 => Some(key(Key::Backspace, modifiers)),
            40 => Some(Event::Text("SELECT '输入正常' AS value;".into())),
            _ => None,
        };
        input.events.push(Event::ModifiersChanged(modifiers));
        if let Some(event) = event {
            input.events.push(event);
        }
        if self.frame <= 60 {
            ctx.request_repaint_after_for(std::time::Duration::from_millis(16), input.viewport_id);
        }
    }
}
