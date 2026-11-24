//! Opt-in native captures use an owned SQLite file and this app's widget events.
use super::*;
use egui::{Context, Event, Id, RawInput};
impl TomatoApp {
    pub(in crate::app) fn preview_data_export(&mut self, ctx: &Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("需要隔离配置"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("tomato-export-fixture-"))
                && self.store.connections.is_empty()
                && std::env::var_os("TOMATO_UI_CAPTURE").is_some(),
            "导出夹具必须使用独立空配置"
        );
        let mut connection = Connection::new("SQLITE");
        connection.name = "导出验收".into();
        connection.set(
            "database",
            profile.join("owned.sqlite").to_string_lossy().as_ref(),
        );
        use tomato_core::workbench::{DatabaseAction, DatabaseOutput, PageRequest, TableId};
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("运行时未初始化"))?;
        let fields = std::env::var("TOMATO_EXPORT_FIELD_COUNT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(4)
            .clamp(4, 256);
        let extra_columns = (4..fields)
            .map(|index| {
                let name = if index == 5 {
                    format!("long_field_{}", "column_name_".repeat(12))
                } else {
                    format!("field_{index:04}")
                };
                format!(",{name} TEXT")
            })
            .collect::<String>();
        let page=runtime.block_on(async {
            let mut db=tomato_services::Databases::default();
            db.query(&connection,&format!("CREATE TABLE records(id INTEGER PRIMARY KEY,name TEXT,note TEXT,data BLOB{extra_columns}); WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<5003) INSERT INTO records(id,name,note,data) SELECT x,'第'||x||'行',CASE WHEN x%3=0 THEN NULL WHEN x%3=1 THEN '' ELSE 'NULL' END, x'00FF11' FROM n")).await?;
            let output=db.workbench(&connection,DatabaseAction::Page {request:PageRequest {table:TableId {schema:"main".into(),name:"records".into()},offset:0,limit:1000,sort:None,query: Default::default(),}}).await?;
            match output {DatabaseOutput::Page(page)=>Ok(page),_=>anyhow::bail!("表分页返回类型错误")}
        })?;
        let database = connection.text("database").to_owned();
        let snapshot = Snapshot {
            schema: tomato_core::data_export::Schema {
                table: page.definition.table,
                dialect: "SQLITE".into(),
                columns: page.definition.columns,
            },
            rows: page.rows,
        };
        let mut draft = Draft::new(snapshot, connection, database, true, ctx);
        let stage = std::env::var("TOMATO_EXPORT_STAGE").unwrap_or_else(|_| "scope".into());
        draft.scope = matches!(stage.as_str(), "scope" | "current" | "all");
        let format = std::env::var("TOMATO_EXPORT_FORMAT").unwrap_or_else(|_| "txt".into());
        draft.options.format = tomato_core::data_export::Format::ALL
            .into_iter()
            .find(|f| f.extension() == format)
            .ok_or_else(|| anyhow::anyhow!("导出格式无效"))?;
        if draft.options.format == tomato_core::data_export::Format::Csv {
            draft.options.delimiter = ',';
        }
        draft.path = profile
            .join(format!("export.{}", draft.options.format.extension()))
            .to_string_lossy()
            .into_owned();
        draft.step = match stage.as_str() {
            "fields" | "fields-bottom" => 2,
            "options" => 3,
            _ => 0,
        };
        draft.all = stage == "all";
        self.data_export = Some(draft);
        self.quit_approved = true;
        ctx.data_mut(|data| data.insert_temp(Id::new("owned-export-fixture"), true));
        if matches!(stage.as_str(), "current" | "all") {
            ctx.add_plugin(Input {
                frame: 0,
                state: None,
            });
        } else if stage == "fields-bottom" {
            ctx.add_plugin(FieldScroll(0));
        }
        Ok(())
    }
}
pub(super) fn capture(ui: &egui::Ui, viewport: egui::ViewportId, draft: &Draft) {
    if !ui
        .ctx()
        .data(|data| data.get_temp::<bool>(Id::new("owned-export-fixture")))
        .unwrap_or(false)
    {
        return;
    }
    ui.ctx()
        .data_mut(|data| data.insert_temp(Id::new("tomato-capture-viewport"), viewport));
    ui.ctx().data_mut(|data| {
        data.insert_temp(
            Id::new("owned-export-fixture-step"),
            (draft.scope, draft.step, draft.all),
        )
    });
    if let Some(image) = ui.input(|i| {
        i.events.iter().find_map(|e| {
            if let Event::Screenshot {
                viewport_id, image, ..
            } = e
                && *viewport_id == viewport
            {
                Some(image.clone())
            } else {
                None
            }
        })
    }) {
        ui.ctx()
            .data_mut(|data| data.insert_temp(Id::new("tomato-capture-image"), image));
    }
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
        let report = draft
            .report
            .as_ref()
            .map(|r| serde_json::json!({"records":r.records,"path":r.path}));
        let proof = serde_json::json!({"ready":true,"scope":draft.scope,"step":draft.step,"format":draft.options.format,"all":draft.all,"currentRows":draft.count,"totalFields":draft.fields.len(),"selectedFields":draft.fields.iter().filter(|selected|**selected).count(),"width":ui.max_rect().width(),"height":ui.max_rect().height(),"running":draft.running(),"error":draft.error,"report":report,"osInputUsed":false,"savedHostsContacted":false,"systemClipboardWritten":false});
        let _ = std::fs::write(path.with_extension("state.json"), proof.to_string());
    }
}
struct FieldScroll(u32);
impl egui::plugin::Plugin for FieldScroll {
    fn debug_name(&self) -> &'static str {
        "owned export fields scroll fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        if input.viewport_id == egui::ViewportId::ROOT {
            return;
        }
        self.0 += 1;
        if self.0 == 8 {
            input.events.extend([
                Event::PointerMoved(egui::pos2(300., 250.)),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(0., -100_000.),
                    modifiers: Default::default(),
                },
            ]);
        }
        if self.0 < 25 {
            ctx.request_repaint();
        }
    }
}
struct Input {
    frame: u32,
    state: Option<(bool, usize, bool)>,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned export native fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        if input.viewport_id == egui::ViewportId::ROOT {
            return;
        }
        let state = ctx.data(|data| {
            data.get_temp::<(bool, usize, bool)>(Id::new("owned-export-fixture-step"))
        });
        if state != self.state {
            self.state = state;
            self.frame = 0;
        }
        self.frame += 1;
        input.focused = true;
        if matches!(self.frame, 12 | 13)
            && self.state.is_some_and(|(scope, step, _)| scope || step < 4)
        {
            let (scope, _, all) = self.state.unwrap();
            let pos = if scope {
                egui::pos2(if all { 63. } else { 160. }, 72.)
            } else {
                egui::pos2(483., 459.)
            };
            input.events.extend([
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: self.frame == 12,
                    modifiers: Default::default(),
                },
            ]);
        }
        if self.frame < 25 || self.state.is_some_and(|(scope, step, _)| scope || step < 4) {
            ctx.request_repaint();
        }
    }
}
