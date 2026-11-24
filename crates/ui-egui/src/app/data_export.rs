//! Navicat-style scope confirmation and five-step native export wizard.
mod fields;
mod fixture;
#[cfg(test)]
mod tests;
mod view;
use super::*;
use tomato_core::data_export::{Options, Snapshot};
use tomato_services::data_export::{Control, Report, Request, Source};

pub(super) struct Draft {
    id: String,
    current: Option<Snapshot>,
    connection: Connection,
    database: String,
    sort: Option<tomato_core::workbench::Sort>,
    table_query: tomato_core::workbench::TableQuery,
    query: Option<String>,
    all_available: bool,
    all: bool,
    scope: bool,
    step: usize,
    path: String,
    options: Options,
    fields: Vec<bool>,
    field_order: Vec<usize>,
    count: usize,
    aliases: Vec<String>,
    pub(super) control: Control,
    receiver: Option<mpsc::Receiver<anyhow::Result<Report>>>,
    metadata: Option<mpsc::Receiver<anyhow::Result<tomato_core::data_export::Schema>>>,
    pub(super) task: Option<tokio::task::JoinHandle<()>>,
    error: Option<String>,
    report: Option<Report>,
    closing: bool,
}
impl Drop for Draft {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
impl Draft {
    fn new(
        snapshot: Snapshot,
        connection: Connection,
        database: String,
        all_available: bool,
        ctx: &egui::Context,
    ) -> Self {
        let fields = vec![true; snapshot.schema.columns.len()];
        let field_order = (0..snapshot.schema.columns.len()).collect();
        let count = snapshot.rows.len();
        let aliases = snapshot
            .schema
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                if c.name
                    .chars()
                    .map(|ch| if ch.is_ascii() { 1 } else { 2 })
                    .sum::<usize>()
                    <= 10
                {
                    c.name.clone()
                } else {
                    format!("FIELD{}", i + 1)
                }
            })
            .collect();
        let repaint = ctx.clone();
        Self {
            id: Connection::new("TOOL").id,
            current: Some(snapshot),
            connection,
            database,
            sort: None,
            table_query: Default::default(),
            query: None,
            all_available,
            all: false,
            scope: true,
            step: 0,
            path: String::new(),
            options: Options::default(),
            fields,
            field_order,
            count,
            aliases,
            control: Control::new(move || repaint.request_repaint()),
            receiver: None,
            metadata: None,
            task: None,
            error: None,
            report: None,
            closing: false,
        }
    }
    fn running(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }
    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.metadata.is_none(), "正在读取表结构，请稍候");
        anyhow::ensure!(!self.path.trim().is_empty(), "请选择导出文件");
        let path = PathBuf::from(&self.path);
        anyhow::ensure!(path.file_name().is_some(), "请选择有效的导出文件名");
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        anyhow::ensure!(
            extension.eq_ignore_ascii_case(self.options.format.extension())
                || (self.options.format == tomato_core::data_export::Format::Html
                    && extension.eq_ignore_ascii_case("htm")),
            "导出文件的扩展名应为 .{}",
            self.options.format.extension()
        );
        anyhow::ensure!(self.fields.iter().any(|v| *v), "请至少选择一个导出字段");
        Ok(())
    }
    fn start(&mut self, runtime: &tokio::runtime::Runtime) -> anyhow::Result<()> {
        self.validate()?;
        self.options.fields = self
            .field_order
            .iter()
            .copied()
            .filter(|&i| self.fields[i])
            .collect();
        let snapshot = self
            .current
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("当前数据已释放，请重新打开导出向导"))?;
        self.options.expected_columns = snapshot
            .schema
            .columns
            .iter()
            .map(|c| c.name.clone())
            .collect();
        self.options.field_names = if self.options.format == tomato_core::data_export::Format::Dbase
        {
            self.options
                .fields
                .iter()
                .map(|&i| self.aliases[i].clone())
                .collect()
        } else {
            Vec::new()
        };
        let source = if self.all {
            anyhow::ensure!(
                self.all_available,
                "此结果不能重新查询全部记录，请导出当前结果"
            );
            if let Some(sql) = &self.query {
                Source::Query {
                    connection: self.connection.clone(),
                    database: self.database.clone(),
                    sql: sql.clone(),
                    schema: snapshot.schema.clone(),
                }
            } else {
                Source::Table {
                    connection: self.connection.clone(),
                    database: self.database.clone(),
                    table: snapshot.schema.table.clone(),
                    sort: self.sort.clone(),
                    query: self.table_query.clone(),
                }
            }
        } else {
            Source::Current(
                self.current
                    .take()
                    .ok_or_else(|| anyhow::anyhow!("当前记录已释放，请重新打开导出向导"))?,
            )
        };
        let request = Request {
            source,
            path: PathBuf::from(&self.path),
            options: self.options.clone(),
        };
        if self.all
            && let Some(snapshot) = &mut self.current
        {
            snapshot.rows = Vec::new();
        }
        self.control.reset(1);
        self.control.log("开始导出");
        let control = self.control.clone();
        let handle = runtime.handle().clone();
        let repaint = self.control.clone();
        let (sender, receiver) = mpsc::channel();
        self.task = Some(runtime.spawn_blocking(move || {
            let result = tomato_services::data_export::run(request, control, handle);
            let _ = sender.send(result);
            repaint.log("导出任务结束");
        }));
        self.receiver = Some(receiver);
        self.step = 4;
        self.error = None;
        self.report = None;
        Ok(())
    }
    fn poll(&mut self) {
        if let Some(receiver) = &self.metadata {
            match receiver.try_recv() {
                Ok(result) => {
                    self.metadata = None;
                    match result {
                        Ok(schema) => {
                            self.fields = vec![true; schema.columns.len()];
                            self.field_order = (0..schema.columns.len()).collect();
                            self.aliases = schema
                                .columns
                                .iter()
                                .enumerate()
                                .map(|(i, c)| {
                                    if c.name
                                        .chars()
                                        .map(|ch| if ch.is_ascii() { 1 } else { 2 })
                                        .sum::<usize>()
                                        <= 10
                                    {
                                        c.name.clone()
                                    } else {
                                        format!("FIELD{}", i + 1)
                                    }
                                })
                                .collect();
                            self.current = Some(Snapshot {
                                schema,
                                rows: Vec::new(),
                            });
                        }
                        Err(error) => self.error = Some(format!("加载表结构失败：{error:#}")),
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.metadata = None;
                    self.error = Some("加载表结构任务异常结束".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(receiver) = &self.receiver {
            match receiver.try_recv() {
                Ok(result) => {
                    self.receiver = None;
                    match result {
                        Ok(report) => self.report = Some(report),
                        Err(error) => self.error = Some(format!("{error:#}")),
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.receiver = None;
                    if self.running() {
                        self.error = Some("导出任务异常结束".into());
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
}
impl TomatoApp {
    pub(super) fn open_object_data_export(
        &mut self,
        connection: Connection,
        namespace: String,
        table: tomato_core::workbench::TableId,
    ) {
        if self.data_export.is_some() {
            return;
        }
        let database = tabs::workbench::session_database(&connection, &namespace).to_owned();
        let snapshot = Snapshot {
            schema: tomato_core::data_export::Schema {
                table: table.clone(),
                dialect: connection.kind().into(),
                columns: Vec::new(),
            },
            rows: Vec::new(),
        };
        let mut draft = Draft::new(
            snapshot,
            connection.clone(),
            database.clone(),
            true,
            &self.context,
        );
        draft.scope = false;
        draft.all = true;
        let Some(runtime) = &self.runtime else {
            return;
        };
        let control = draft.control.clone();
        let repaint = self.context.clone();
        let (sender, receiver) = mpsc::channel();
        draft.task = Some(runtime.spawn(async move {
            let result =
                tomato_services::database::export::schema(&connection, &database, &table, &control)
                    .await;
            let _ = sender.send(result);
            repaint.request_repaint();
        }));
        draft.metadata = Some(receiver);
        self.data_export = Some(draft);
    }
    pub(super) fn open_data_export(&mut self, id: &str, result: Option<usize>) {
        if self.data_export.is_some() {
            return;
        }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return;
        };
        let Panel::Sql {
            connection,
            workbench,
            results,
            ..
        } = &mut tab.panel
        else {
            return;
        };
        let snapshot = if let Some(index) = result {
            let Some(result) = results.get(index) else {
                return;
            };
            if let Some(state) = workbench.query_results.get_mut(index)
                && let Some(editing) = &mut state.editing
            {
                editing.release(&self.context);
                if let Err(error) = editing.finish(result) {
                    self.error = Some(error.to_string());
                    return;
                }
            }
            use tomato_core::{
                data_export::Schema,
                workbench::{Cell, Column, TableId},
            };
            let projection = workbench.query_results.get(index);
            let columns = result
                .columns
                .iter()
                .map(|name| Column {
                    name: name.clone(),
                    data_type: "TEXT".into(),
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
            let count = projection.map_or(result.rows.len(), |state| state.visible_rows());
            let rows = (0..count)
                .filter(|&i| {
                    let row = projection.map_or(i, |state| state.row(i));
                    !projection
                        .and_then(|s| s.editing.as_ref())
                        .is_some_and(|editing| editing.deleted.contains(&row))
                })
                .map(|i| {
                    let row = projection.map_or(i, |state| state.row(i));
                    (0..result.columns.len())
                        .map(|c| {
                            projection
                                .map_or_else(
                                    || result.value(row, c),
                                    |state| state.display_value(result, row, c),
                                )
                                .map_or(Cell::Null, |text| Cell::Text(text.into()))
                        })
                        .collect()
                })
                .collect();
            Ok(Snapshot {
                schema: Schema {
                    table: TableId {
                        schema: String::new(),
                        name: "result".into(),
                    },
                    dialect: connection.kind().into(),
                    columns,
                },
                rows,
            })
        } else {
            panels::export_table_snapshot(&self.context, workbench, connection.kind())
        };
        match snapshot {
            Ok(snapshot) => {
                let database =
                    tabs::workbench::session_database(connection, &workbench.schema).to_owned();
                let mut draft = Draft::new(
                    snapshot,
                    connection.clone(),
                    database,
                    result.is_none(),
                    &self.context,
                );
                draft.sort = workbench.sort.clone();
                draft.table_query = workbench.query.clone();
                if let Some(index) = result {
                    if let Some(state) = workbench.query_results.get(index) {
                        let visible = state.visible_columns();
                        if !visible.is_empty() {
                            draft.field_order = visible.as_ref().clone();
                            draft
                                .field_order
                                .extend((0..draft.fields.len()).filter(|i| !visible.contains(i)));
                            for (i, field) in draft.fields.iter_mut().enumerate() {
                                *field = visible.contains(&i);
                            }
                        }
                    }
                    if let Some(script) = &workbench.script
                        && let Some(statement) = script
                            .statements
                            .iter()
                            .find(|s| s.result == Some(index) && s.select && s.success)
                    {
                        draft.query = Some(script.sql(&statement.range).into());
                        draft.all_available = true;
                    }
                    if let Some(context) = &workbench.result_context {
                        draft.connection = (*context.connection).clone();
                        if let Some(snapshot) = &mut draft.current {
                            snapshot.schema.dialect = draft.connection.kind().into();
                        }
                        draft.database = tabs::workbench::session_database(
                            &context.connection,
                            &context.namespace,
                        )
                        .into();
                    }
                }
                if result.is_none() {
                    for &hidden in &workbench.table_view.projection.hidden {
                        if let Some(field) = draft.fields.get_mut(hidden) {
                            *field = false;
                        }
                    }
                }
                self.data_export = Some(draft);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
    pub(super) fn data_export_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.data_export.take() else {
            return;
        };
        draft.poll();
        if draft.closing && !draft.running() {
            return;
        }
        let viewport = egui::ViewportId::from_hash_of(("data-export", &draft.id, draft.scope));
        let size = if draft.scope {
            [324., 96.]
        } else {
            [640., 480.]
        };
        let mut builder = egui::ViewportBuilder::default()
            .with_title(if draft.scope {
                "确认"
            } else {
                "导出向导"
            })
            .with_inner_size(size)
            .with_resizable(false)
            .with_minimize_button(!draft.scope)
            .with_maximize_button(false);
        if !draft.scope
            && let Some(icon) = icon()
        {
            builder = builder.with_icon(icon);
        }
        let mut action = None;
        ctx.show_viewport_immediate(viewport, builder, |ui, _| {
            action = view::render(ui, &mut draft);
            fixture::capture(ui, viewport, &draft);
        });
        match action {
            Some(view::Action::Scope(all)) => {
                draft.all = all;
                draft.scope = false;
            }
            Some(view::Action::Next) => {
                if draft.step == 0 {
                    draft.step = 1;
                } else if draft.step == 1 {
                    if draft.path.trim().is_empty() {
                        draft.error = Some("请选择导出文件".into());
                    } else {
                        draft.step = 2;
                        draft.error = None;
                    }
                } else if draft.step == 2 {
                    if draft.fields.iter().any(|v| *v) {
                        draft.step = 3;
                        draft.error = None;
                    } else {
                        draft.error = Some("请至少选择一个导出字段".into());
                    }
                }
            }
            Some(view::Action::Back) => {
                draft.step = draft.step.saturating_sub(1);
                draft.error = None;
            }
            Some(view::Action::Pick) => {
                let format = draft.options.format;
                let name = draft
                    .current
                    .as_ref()
                    .map_or("data", |s| s.schema.table.name.as_str());
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("导出文件")
                    .set_file_name(format!("{name}.{}", format.extension()))
                    .add_filter(format.label(), &[format.extension()])
                    .save_file()
                {
                    draft.path = path.to_string_lossy().into_owned();
                    draft.error = None;
                }
            }
            Some(view::Action::Start) => {
                if let Some(runtime) = &self.runtime
                    && let Err(error) = draft.start(runtime)
                {
                    draft.error = Some(error.to_string());
                }
            }
            Some(view::Action::Close) => {
                if draft.running() {
                    draft.control.cancel();
                    draft.closing = true;
                    ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::CancelClose);
                } else {
                    return;
                }
            }
            None => {}
        }
        if draft.running() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.data_export = Some(draft);
    }
}
fn icon() -> Option<std::sync::Arc<egui::IconData>> {
    static ICON: std::sync::OnceLock<Option<std::sync::Arc<egui::IconData>>> =
        std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let image = image::load_from_memory(include_bytes!(
            "../../assets/images/connect/table_export.png"
        ))
        .ok()?
        .into_rgba8();
        Some(std::sync::Arc::new(egui::IconData {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        }))
    })
    .clone()
}
