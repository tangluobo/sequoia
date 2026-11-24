//! The native import wizard owns settings and background jobs, not protocol implementations.
mod fixture;
#[cfg(test)]
mod tests;
mod view;
use super::*;
use egui::Context;
use std::sync::Arc;
use tomato_core::{
    data_import::*,
    workbench::{TableId, TableInfo},
};
use tomato_services::{
    data_import::{Catalog, Control, Prepared},
    database::import as service,
};
enum Event {
    Prepared(Arc<Prepared>),
    Tables(Vec<TableInfo>),
    Mapped(Vec<Target>),
    Odbc(Catalog),
    Done(Report),
}
fn saved_target<'a>(
    profile: &'a Profile,
    source: &SourceTable,
    index: usize,
) -> Option<&'a Target> {
    profile
        .targets
        .iter()
        .find(|t| {
            t.source_key
                .as_ref()
                .is_some_and(|(origin, name)| origin == &source.origin && name == &source.name)
        })
        .or_else(|| {
            profile
                .targets
                .iter()
                .find(|t| t.source_key.is_none() && t.source == index)
        })
}
pub(super) struct Draft {
    id: String,
    connection: Connection,
    namespace: String,
    database: String,
    preferred: Option<TableId>,
    source: Source,
    options: Options,
    files: String,
    selected_tables: String,
    delimiter: String,
    quote: String,
    null_value: String,
    last_row: String,
    step: usize,
    prepared: Option<Arc<Prepared>>,
    targets: Vec<Target>,
    known: Vec<TableInfo>,
    selected: usize,
    preview: usize,
    odbc: Catalog,
    pub(super) task: Option<tokio::task::JoinHandle<()>>,
    receiver: Option<mpsc::Receiver<anyhow::Result<Event>>>,
    pub(super) control: Control,
    error: Option<String>,
    report: Option<Report>,
    closing: bool,
    confirming: bool,
    preview_only: bool,
    profile: Option<Profile>,
}
impl Drop for Draft {
    fn drop(&mut self) {
        self.control.cancel();
    }
}
impl Draft {
    fn new(
        connection: Connection,
        namespace: String,
        preferred: Option<TableId>,
        ctx: &Context,
    ) -> Self {
        let database = tabs::workbench::session_database(&connection, &namespace).to_owned();
        let repaint = ctx.clone();
        Self {
            id: tomato_core::collaboration::new_id(),
            connection,
            namespace,
            database,
            preferred,
            source: Source::default(),
            options: Options::default(),
            files: String::new(),
            selected_tables: String::new(),
            delimiter: "\\t".into(),
            quote: "\"".into(),
            null_value: "\\N".into(),
            last_row: String::new(),
            step: 0,
            prepared: None,
            targets: Vec::new(),
            known: Vec::new(),
            selected: 0,
            preview: 0,
            odbc: Catalog::default(),
            task: None,
            receiver: None,
            control: Control::new(move || repaint.request_repaint()),
            error: None,
            report: None,
            closing: false,
            confirming: false,
            preview_only: false,
            profile: None,
        }
    }
    fn busy(&self) -> bool {
        self.receiver.is_some()
    }
    fn configure(&mut self) -> anyhow::Result<()> {
        self.source.files = self
            .files
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| PathBuf::from(s.trim_matches('"')))
            .collect();
        self.source.tables = self
            .selected_tables
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if matches!(self.source.format, Format::Text | Format::Csv) {
            let delimiter = if self.delimiter == "\\t" {
                "\t"
            } else {
                &self.delimiter
            };
            anyhow::ensure!(
                delimiter.chars().count() == 1,
                "请输入一个字段分隔符（制表符可填写 \\t）"
            );
            self.source.delimiter = delimiter.chars().next().unwrap();
            anyhow::ensure!(
                self.quote.chars().count() <= 1,
                "文本限定符应为空或一个字符"
            );
            self.source.quote = self.quote.chars().next();
        }
        self.source.null_text = if self.null_value.is_empty() {
            None
        } else {
            Some(self.null_value.clone())
        };
        self.source.last_row = if self.last_row.trim().is_empty() {
            None
        } else {
            Some(self.last_row.parse()?)
        };
        self.source.validate()
    }
    fn request(&self) -> anyhow::Result<service::Request> {
        let prepared = self
            .prepared
            .clone()
            .ok_or_else(|| anyhow::anyhow!("请先读取源数据"))?;
        Ok(service::Request {
            connection: self.connection.clone(),
            database: self.database.clone(),
            prepared,
            targets: self.targets.clone(),
            options: self.options.clone(),
        })
    }
    fn work(
        &mut self,
        runtime: &tokio::runtime::Runtime,
        work: impl FnOnce() -> anyhow::Result<Event> + Send + 'static,
    ) {
        self.error = None;
        self.control.reset(1);
        let repaint = self.control.clone();
        let (tx, rx) = mpsc::channel();
        self.receiver = Some(rx);
        self.task = Some(runtime.spawn_blocking(move || {
            let _ = tx.send(work());
            repaint.log("任务结束");
        }));
    }
    fn prepare(&mut self, runtime: &tokio::runtime::Runtime) -> anyhow::Result<()> {
        self.configure()?;
        self.prepared = None;
        self.targets.clear();
        let source = self.source.clone();
        let control = self.control.clone();
        self.work(runtime, move || {
            Ok(Event::Prepared(Arc::new(
                tomato_services::data_import::prepare(source, &control)?,
            )))
        });
        Ok(())
    }
    fn load_tables(&mut self, runtime: &tokio::runtime::Runtime) {
        let c = self.connection.clone();
        let db = self.database.clone();
        let schema = self.namespace.clone();
        let control = self.control.clone();
        let handle = runtime.handle().clone();
        self.work(runtime, move || {
            Ok(Event::Tables(
                handle.block_on(service::tables(&c, &db, &schema, &control))?,
            ))
        });
    }
    fn map(&mut self, runtime: &tokio::runtime::Runtime) {
        let c = self.connection.clone();
        let db = self.database.clone();
        let mut targets = self.targets.clone();
        let prepared = self.prepared.clone().unwrap();
        let control = self.control.clone();
        let handle = runtime.handle().clone();
        self.work(runtime, move || {
            for target in &mut targets {
                if !target.selected {
                    continue;
                }
                control.check()?;
                if !target.create && target.expected.is_none() {
                    let definition =
                        handle.block_on(service::definition(&c, &db, &target.table, &control))?;
                    let source = target.source;
                    let table = target.table.clone();
                    *target = service::defaults(
                        source,
                        &prepared.tables[source],
                        table,
                        Some(definition),
                    );
                }
            }
            Ok(Event::Mapped(targets))
        });
    }
    fn start(&mut self, runtime: &tokio::runtime::Runtime) -> anyhow::Result<()> {
        let request = self.request()?;
        service::validate(&request)?;
        let control = self.control.clone();
        let handle = runtime.handle().clone();
        self.step = 7;
        self.report = None;
        self.work(runtime, move || {
            Ok(Event::Done(service::run(request, control, handle)?))
        });
        Ok(())
    }
    fn save_profile(&mut self) -> anyhow::Result<()> {
        self.configure()?;
        if let Some(mut path) = rfd::FileDialog::new()
            .set_title("保存导入配置")
            .add_filter("导入配置", &["timport"])
            .save_file()
        {
            path.set_extension("timport");
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("配置目录无效"))?;
            let profile = Profile::redacted(&self.source, &self.targets, &self.options);
            let bytes = serde_json::to_vec_pretty(&profile)?;
            anyhow::ensure!(bytes.len() <= 1024 * 1024, "导入配置超过 1 MiB");
            use std::io::Write;
            let mut file = tempfile::NamedTempFile::new_in(parent)?;
            file.write_all(&bytes)?;
            file.as_file().sync_all()?;
            file.persist(path)?;
        }
        Ok(())
    }
    fn load_profile(&mut self) -> anyhow::Result<()> {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("加载导入配置")
            .add_filter("导入配置", &["timport"])
            .pick_file()
        {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::fs::File::open(path)?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            anyhow::ensure!(bytes.len() <= 1024 * 1024, "导入配置超过 1 MiB");
            let profile: Profile = serde_json::from_slice(&bytes)?;
            anyhow::ensure!(profile.version == 1, "导入配置版本不受支持");
            self.source = profile.source.without_secrets();
            self.options = profile.options.clone();
            self.files = self
                .source
                .files
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join("\n");
            self.delimiter = if self.source.delimiter == '\t' {
                "\\t".into()
            } else {
                self.source.delimiter.to_string()
            };
            self.quote = self.source.quote.map_or(String::new(), |q| q.to_string());
            self.last_row = self
                .source
                .last_row
                .map_or(String::new(), |r| r.to_string());
            self.null_value = self.source.null_text.clone().unwrap_or_default();
            self.selected_tables = self.source.tables.join("\n");
            self.profile = Some(profile);
            self.prepared = None;
            self.targets.clear();
            self.step = 1;
        }
        Ok(())
    }
}
impl TomatoApp {
    pub(super) fn open_data_import(
        &mut self,
        connection: Connection,
        namespace: String,
        table: Option<TableId>,
    ) {
        if self.data_import.is_none() {
            self.data_import = Some(Draft::new(connection, namespace, table, &self.context));
        }
    }
    pub(super) fn open_table_data_import(&mut self, id: &str) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == id) else {
            return;
        };
        if tab.dirty() {
            self.error = Some("请先保存或取消当前表中的修改，再打开导入向导".into());
            return;
        }
        let Panel::Sql {
            connection,
            workbench,
            ..
        } = &tab.panel
        else {
            return;
        };
        if workbench.transaction {
            self.error = Some("请先提交或回滚当前表的事务，再打开导入向导".into());
            return;
        }
        let c = connection.clone();
        let schema = workbench.schema.clone();
        let table = workbench.table.clone();
        self.open_data_import(c, schema, table);
    }
    pub(super) fn data_import_dialog(&mut self, ctx: &Context) {
        let Some(mut draft) = self.data_import.take() else {
            return;
        };
        if let Some(receiver) = &draft.receiver {
            let result = match receiver.try_recv() {
                Ok(value) => Some(value),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err(anyhow::anyhow!("导入任务异常结束")))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                draft.receiver = None;
                draft.task = None;
                match result {
                    Ok(Event::Prepared(source)) => {
                        draft.prepared = Some(source);
                        if !draft.closing
                            && !draft.preview_only
                            && let Some(runtime) = &self.runtime
                        {
                            draft.load_tables(runtime)
                        }
                    }
                    Ok(Event::Tables(tables)) => {
                        draft.known = tables;
                        let prepared = draft.prepared.as_ref().unwrap();
                        let mut unmatched = false;
                        draft.targets = prepared
                            .tables
                            .iter()
                            .enumerate()
                            .map(|(i, table)| {
                                let preferred = if i == 0 {
                                    draft.preferred.clone()
                                } else {
                                    None
                                };
                                let matched = preferred.or_else(|| {
                                    draft
                                        .known
                                        .iter()
                                        .find(|t| t.table.name.eq_ignore_ascii_case(&table.name))
                                        .map(|t| t.table.clone())
                                });
                                let create = matched.is_none();
                                let target = matched.unwrap_or_else(|| TableId {
                                    schema: draft.namespace.clone(),
                                    name: table.name.chars().take(63).collect(),
                                });
                                let mut t = service::defaults(i, table, target, None);
                                t.create = create;
                                t.selected = draft.preferred.is_none() || i == 0;
                                if let Some(saved) = draft
                                    .profile
                                    .as_ref()
                                    .and_then(|p| saved_target(p, table, i))
                                {
                                    t.table.name.clone_from(&saved.table.name);
                                    t.create = saved.create;
                                    t.selected = saved.selected;
                                } else if draft
                                    .profile
                                    .as_ref()
                                    .is_some_and(|p| !p.targets.is_empty())
                                {
                                    t.selected = false;
                                    unmatched = true;
                                }
                                t
                            })
                            .collect();
                        if unmatched {
                            draft.error = Some(
                                "部分源数据表与导入配置不一致，已取消勾选，请重新确认目标表和字段"
                                    .into(),
                            );
                        }
                        draft.step = 3;
                        draft.selected = 0;
                    }
                    Ok(Event::Mapped(mut targets)) => {
                        if let Some(profile) = draft.profile.take() {
                            for target in &mut targets {
                                let source =
                                    &draft.prepared.as_ref().unwrap().tables[target.source];
                                if let Some(saved) = saved_target(&profile, source, target.source)
                                    .filter(|t| t.table.name == target.table.name)
                                {
                                    let fields = &source.fields;
                                    target.fields = saved.fields.clone();
                                    for f in &mut target.fields {
                                        if let Some(name) = &f.source_name {
                                            f.source = fields.iter().position(|s| &s.name == name);
                                            if f.source.is_none() {
                                                f.enabled = false;
                                                draft.error=Some("部分预设源字段不存在，已取消相应映射，请检查字段页".into());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        draft.targets = targets;
                        draft.step = 4;
                        draft.selected = draft.targets.iter().position(|t| t.selected).unwrap_or(0);
                    }
                    Ok(Event::Odbc(catalog)) => draft.odbc = catalog,
                    Ok(Event::Done(report)) => {
                        if !report.changed.is_empty() {
                            self.refresh_transfer_dependents(
                                &draft.connection,
                                &draft.namespace,
                                &report.changed,
                            );
                        }
                        draft.error = report.error.clone();
                        draft.report = Some(report);
                    }
                    Err(error) => draft.error = Some(error.to_string()),
                }
            }
        }
        if draft.closing && !draft.busy() {
            return;
        }
        let viewport = egui::ViewportId::from_hash_of(("data-import", &draft.id));
        let mut builder = egui::ViewportBuilder::default()
            .with_title("导入向导")
            .with_inner_size([680., 480.])
            .with_resizable(false)
            .with_maximize_button(false);
        if let Some(icon) = icon() {
            builder = builder.with_icon(icon)
        }
        let mut action = None;
        ctx.show_viewport_immediate(viewport, builder, |ui, _| {
            action = view::render(ui, &mut draft);
            fixture::capture(ui, viewport, &draft);
        });
        let result = (|| -> anyhow::Result<()> {
            match action {
                Some(view::Action::Next) => match draft.step {
                    0 => draft.step = 1,
                    1 => {
                        draft.configure()?;
                        draft.step = 2;
                    }
                    2 => {
                        draft.preview_only = false;
                        if let Some(runtime) = &self.runtime {
                            draft.prepare(runtime)?;
                        }
                    }
                    3 => {
                        anyhow::ensure!(
                            draft.targets.iter().any(|t| t.selected),
                            "请选择源表并指定目标表"
                        );
                        if let Some(runtime) = &self.runtime {
                            draft.map(runtime)
                        }
                    }
                    4 => {
                        let mut mapping = draft.request()?;
                        mapping.options.mode = Mode::Append;
                        service::validate(&mapping)?;
                        draft.step = 5;
                    }
                    5 => {
                        service::validate(&draft.request()?)?;
                        draft.step = 6;
                    }
                    _ => {}
                },
                Some(view::Action::Back) => {
                    draft.step = draft.step.saturating_sub(1);
                    draft.error = None;
                }
                Some(view::Action::Pick) => {
                    if let Some(files) = rfd::FileDialog::new()
                        .set_title("选择导入文件")
                        .add_filter(
                            draft.source.format.label(),
                            draft.source.format.extensions(),
                        )
                        .pick_files()
                    {
                        draft.files = files
                            .iter()
                            .map(|p| p.to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("\n");
                    }
                }
                Some(view::Action::Preview) => {
                    draft.preview_only = true;
                    if let Some(runtime) = &self.runtime {
                        draft.prepare(runtime)?;
                    }
                }
                Some(view::Action::OdbcSources) => {
                    let bits = draft.source.odbc.use_32bit;
                    let control = draft.control.clone();
                    if let Some(runtime) = &self.runtime {
                        draft.work(runtime, move || {
                            Ok(Event::Odbc(tomato_services::data_import::odbc_sources(
                                bits, &control,
                            )?))
                        });
                    }
                }
                Some(view::Action::OdbcTables) => {
                    let config = draft.source.odbc.clone();
                    let control = draft.control.clone();
                    if let Some(runtime) = &self.runtime {
                        draft.work(runtime, move || {
                            Ok(Event::Odbc(tomato_services::data_import::discover_odbc(
                                config, &control,
                            )?))
                        });
                    }
                }
                Some(view::Action::Start) => {
                    service::validate(&draft.request()?)?;
                    if matches!(draft.options.mode, Mode::Copy | Mode::Delete) {
                        draft.confirming = true
                    } else if let Some(runtime) = &self.runtime {
                        draft.start(runtime)?;
                    }
                }
                Some(view::Action::Confirmed) => {
                    draft.confirming = false;
                    if let Some(runtime) = &self.runtime {
                        draft.start(runtime)?;
                    }
                }
                Some(view::Action::SaveProfile) => draft.save_profile()?,
                Some(view::Action::LoadProfile) => draft.load_profile()?,
                Some(view::Action::SaveLog) => {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("保存导入日志")
                        .set_file_name("import-log.txt")
                        .add_filter("文本日志", &["txt"])
                        .save_file()
                    {
                        let text = draft
                            .control
                            .read(|p| p.logs.iter().cloned().collect::<Vec<_>>().join("\n"))
                            .unwrap_or_default();
                        tomato_services::files::save_text(&path, &text)?;
                    }
                }
                Some(view::Action::Close) => {
                    if draft.busy() {
                        draft.control.cancel();
                        draft.closing = true;
                        ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::CancelClose);
                    } else {
                        draft.closing = true;
                    }
                }
                None => {}
            }
            Ok(())
        })();
        if let Err(error) = result {
            draft.error = Some(error.to_string());
        }
        if draft.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if !draft.closing || draft.busy() {
            self.data_import = Some(draft);
        }
    }
}
fn icon() -> Option<Arc<egui::IconData>> {
    static ICON: std::sync::OnceLock<Option<Arc<egui::IconData>>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        let image = image::load_from_memory(include_bytes!(
            "../../assets/images/connect/table_import.png"
        ))
        .ok()?
        .into_rgba8();
        Some(Arc::new(egui::IconData {
            width: image.width(),
            height: image.height(),
            rgba: image.into_raw(),
        }))
    })
    .clone()
}
