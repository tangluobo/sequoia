//! Opt-in native widget probes. Every file belongs to an empty fixture profile.
use super::*;
use egui::{Event, Id, RawInput};
use tomato_core::workbench::{Cell, Column};
impl TomatoApp {
    pub(in crate::app) fn preview_data_import(&mut self, ctx: &Context) -> anyhow::Result<()> {
        let profile = std::env::var_os("TOMATO_DATA_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("需要隔离配置"))?;
        anyhow::ensure!(
            profile
                .file_name()
                .is_some_and(|p| p.to_string_lossy().starts_with("tomato-import-fixture-"))
                && self.store.connections.is_empty()
                && std::env::var_os("TOMATO_UI_CAPTURE").is_some(),
            "导入夹具必须使用独立空配置"
        );
        let key = std::env::var("TOMATO_IMPORT_FORMAT").unwrap_or_else(|_| "txt".into());
        let format = match key.as_str() {
            "dbf" => Format::Dbase,
            "db" => Format::Paradox,
            "txt" => Format::Text,
            "csv" => Format::Csv,
            "xls" | "xlsx" => Format::Excel,
            "xml" => Format::Xml,
            "json" => Format::Json,
            "mdb" | "accdb" => Format::Access,
            "odbc" => Format::Odbc,
            _ => anyhow::bail!("导入夹具格式无效"),
        };
        let mut c = Connection::new("SQLITE");
        c.name = "导入验收".into();
        c.set(
            "database",
            profile.join("owned.sqlite").to_string_lossy().as_ref(),
        );
        let runtime = self
            .runtime
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("运行时未初始化"))?;
        runtime.block_on(async {
            tomato_services::Databases::default()
                .query(
                    &c,
                    "CREATE TABLE records(id INTEGER PRIMARY KEY,note TEXT,postal TEXT)",
                )
                .await
        })?;
        let mut draft = Draft::new(
            c,
            "main".into(),
            Some(TableId {
                schema: "main".into(),
                name: "records".into(),
            }),
            ctx,
        );
        draft.source.format = format;
        let stage = std::env::var("TOMATO_IMPORT_STAGE").unwrap_or_else(|_| "formats".into());
        if stage != "formats" {
            let path = profile.join(format!("input.{key}"));
            if let Some(external) = std::env::var_os("TOMATO_IMPORT_EXTERNAL") {
                draft.files = PathBuf::from(external).to_string_lossy().into_owned();
                draft.source.header_row = 0;
                draft.source.first_row = 1;
            } else if format == Format::Paradox {
                paradox(&path)?;
                draft.files = path.to_string_lossy().into_owned();
                draft.source.encoding = "自动".into();
                draft.source.first_row = 1;
                draft.source.header_row = 0;
            } else {
                let export = match key.as_str() {
                    "dbf" => tomato_core::data_export::Format::Dbase,
                    "txt" => tomato_core::data_export::Format::Text,
                    "csv" | "odbc" => tomato_core::data_export::Format::Csv,
                    "xls" => tomato_core::data_export::Format::Xls,
                    "xlsx" => tomato_core::data_export::Format::Xlsx,
                    "xml" => tomato_core::data_export::Format::Xml,
                    "json" => tomato_core::data_export::Format::Json,
                    "accdb" => tomato_core::data_export::Format::Access,
                    _ => anyhow::bail!("该夹具需要外部源文件"),
                };
                let path = if format == Format::Odbc {
                    profile.join("input.csv")
                } else {
                    path
                };
                let columns = [("id", "INTEGER"), ("note", "TEXT"), ("postal", "TEXT")]
                    .into_iter()
                    .map(|(name, typ)| Column {
                        name: name.into(),
                        data_type: typ.into(),
                        nullable: true,
                        default: None,
                        primary_key: 0,
                        generated: false,
                        auto_increment: false,
                        binary: false,
                        type_schema: String::new(),
                        type_name: String::new(),
                    })
                    .collect();
                let snapshot = tomato_core::data_export::Snapshot {
                    schema: tomato_core::data_export::Schema {
                        table: TableId {
                            schema: "main".into(),
                            name: "records".into(),
                        },
                        dialect: "SQLITE".into(),
                        columns,
                    },
                    rows: (1..=2305)
                        .map(|i| {
                            vec![
                                Cell::Integer(i),
                                Cell::Text(format!("第{i}行")),
                                Cell::Text("00123".into()),
                            ]
                        })
                        .collect(),
                };
                let options = tomato_core::data_export::Options {
                    format: export,
                    fields: vec![0, 1, 2],
                    delimiter: if matches!(format, Format::Csv | Format::Odbc) {
                        ','
                    } else {
                        '\t'
                    },
                    ..Default::default()
                };
                tomato_services::data_export::run(
                    tomato_services::data_export::Request {
                        source: tomato_services::data_export::Source::Current(snapshot),
                        path: path.clone(),
                        options,
                    },
                    Control::default(),
                    runtime.handle().clone(),
                )?;
                draft.files = path.to_string_lossy().into_owned();
                if matches!(
                    format,
                    Format::Dbase | Format::Xml | Format::Json | Format::Access
                ) {
                    draft.source.header_row = 0;
                    draft.source.first_row = 1;
                }
                if format == Format::Dbase {
                    draft.source.encoding = "自动".into();
                }
                if matches!(format, Format::Csv | Format::Odbc) {
                    draft.delimiter = ",".into();
                }
                if format == Format::Odbc {
                    draft.source.odbc.use_32bit = true;
                    draft.source.first_row = 1;
                    draft.source.odbc.connection_string = format!(
                        "Driver={{Microsoft Text Driver (*.txt; *.csv)}};DBQ={};Extensions=csv;ReadOnly=True;",
                        profile.display()
                    );
                    draft.source.odbc.tables = vec![OdbcTable {
                        catalog: String::new(),
                        schema: String::new(),
                        name: "input.csv".into(),
                    }];
                    std::fs::write(
                        profile.join("schema.ini"),
                        "[input.csv]\r\nFormat=CSVDelimited\r\nColNameHeader=True\r\nCharacterSet=65001\r\nMaxScanRows=0\r\nCol1=id Long\r\nCol2=note Text Width 80\r\nCol3=postal Text Width 20\r\n",
                    )?;
                }
            }
            if let Ok(password) = std::env::var("TOMATO_IMPORT_PASSWORD") {
                draft.source.access_password = password;
            }
            ctx.add_plugin(Input {
                frame: 0,
                state: None,
                stop: match stage.as_str() {
                    "source" => 1,
                    "options" => 2,
                    "targets" => 3,
                    "fields" => 4,
                    "mode" => 5,
                    "advanced" => 6,
                    _ => 7,
                },
            });
        }
        self.data_import = Some(draft);
        self.quit_approved = true;
        ctx.data_mut(|data| data.insert_temp(Id::new("owned-import-fixture"), true));
        Ok(())
    }
}
fn paradox(path: &std::path::Path) -> anyhow::Result<()> {
    // Documented Paradox 7 alpha/long layout, independent of the reader.
    let record = 73usize;
    let per_block = (1024 - 6) / record;
    let blocks = 2305usize.div_ceil(per_block);
    let mut bytes = vec![0u8; 2048 + blocks * 1024];
    bytes[0..2].copy_from_slice(&(record as u16).to_le_bytes());
    bytes[2..4].copy_from_slice(&2048u16.to_le_bytes());
    bytes[4] = 2;
    bytes[5] = 1;
    bytes[6..10].copy_from_slice(&2305u32.to_le_bytes());
    bytes[0x0e..0x10].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x21..0x23].copy_from_slice(&3u16.to_le_bytes());
    bytes[0x39] = 12;
    bytes[0x6a..0x6c].copy_from_slice(&65001u16.to_le_bytes());
    bytes[0x78..0x7e].copy_from_slice(&[4, 4, 1, 64, 1, 5]);
    let mut cursor = 0x78 + 3 * 6 + 4 + 261;
    for name in ["id", "note", "postal"] {
        bytes[cursor..cursor + name.len()].copy_from_slice(name.as_bytes());
        cursor += name.len() + 1;
    }
    for block in 0..blocks {
        let at = 2048 + block * 1024;
        let count = (2305 - block * per_block).min(per_block);
        let next = if block + 1 == blocks {
            0
        } else {
            block as u16 + 2
        };
        bytes[at..at + 2].copy_from_slice(&next.to_le_bytes());
        bytes[at + 4..at + 6].copy_from_slice(&(((count - 1) * record) as i16).to_le_bytes());
        for row in 0..count {
            let id = (block * per_block + row + 1) as i32;
            let at = at + 6 + row * record;
            let mut number = id.to_be_bytes();
            number[0] ^= 0x80;
            bytes[at..at + 4].copy_from_slice(&number);
            let text = format!("第{id}行");
            bytes[at + 4..at + 4 + text.len()].copy_from_slice(text.as_bytes());
            bytes[at + 68..at + 73].copy_from_slice(b"00123");
        }
    }
    std::fs::write(path, bytes)?;
    Ok(())
}
pub(super) fn capture(ui: &Ui, viewport: egui::ViewportId, draft: &Draft) {
    if !ui
        .ctx()
        .data(|d| d.get_temp::<bool>(Id::new("owned-import-fixture")))
        .unwrap_or(false)
    {
        return;
    }
    let stage = std::env::var("TOMATO_IMPORT_STAGE").unwrap_or_else(|_| "formats".into());
    let step = match stage.as_str() {
        "formats" => 0,
        "source" => 1,
        "options" => 2,
        "targets" => 3,
        "fields" => 4,
        "mode" => 5,
        "advanced" => 6,
        _ => 7,
    };
    let ready = !draft.busy()
        && (draft.error.is_some() || (draft.step == step && (step < 7 || draft.report.is_some())));
    ui.ctx()
        .data_mut(|d| d.insert_temp(Id::new("owned-import-ready"), ready));
    ui.ctx().data_mut(|d| {
        d.insert_temp(Id::new("tomato-capture-viewport"), viewport);
        d.insert_temp(Id::new("owned-import-state"), (draft.step, draft.busy()));
    });
    if let Some(image) = ui.input(|i| {
        i.events.iter().find_map(|e| match e {
            Event::Screenshot {
                viewport_id, image, ..
            } if *viewport_id == viewport => Some(image.clone()),
            _ => None,
        })
    }) {
        ui.ctx()
            .data_mut(|d| d.insert_temp(Id::new("tomato-capture-image"), image));
    }
    if let Some(path) = std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from) {
        let report=draft.report.as_ref().map(|r|serde_json::json!({"read":r.read,"inserted":r.inserted,"updated":r.updated,"deleted":r.deleted,"skipped":r.skipped,"failed":r.failed,"committed":r.committed,"cancelled":r.cancelled,"uncertain":r.uncertain,"errors":r.errors}));
        let state = serde_json::json!({"ready":true,"step":draft.step,"format":draft.source.format,"running":draft.busy(),"error":draft.error,"report":report,"width":ui.max_rect().width(),"height":ui.max_rect().height(),"database":draft.database,"spool":draft.prepared.as_ref().map(|p|p.directory.path()),"tables":draft.targets.len(),"osInputUsed":false,"savedHostsContacted":false,"systemClipboardWritten":false});
        let _ = std::fs::write(path.with_extension("state.json"), state.to_string());
    }
}
struct Input {
    frame: u32,
    state: Option<(usize, bool)>,
    stop: usize,
}
impl egui::plugin::Plugin for Input {
    fn debug_name(&self) -> &'static str {
        "owned import native widget fixture"
    }
    fn input_hook(&mut self, ctx: &Context, input: &mut RawInput) {
        input
            .events
            .retain(|e| matches!(e, Event::Screenshot { .. }));
        if input.viewport_id == egui::ViewportId::ROOT {
            return;
        }
        let state = ctx.data(|d| d.get_temp::<(usize, bool)>(Id::new("owned-import-state")));
        if self.state != state {
            self.state = state;
            self.frame = 0;
        }
        self.frame += 1;
        input.focused = true;
        if let Some((step, false)) = state
            && step < self.stop
            && matches!(self.frame, 12 | 13)
        {
            let pos = egui::pos2(550., 461.);
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
        if self.frame < 25 || state.is_some_and(|(s, b)| s < self.stop || b) {
            ctx.request_repaint();
        }
    }
}
