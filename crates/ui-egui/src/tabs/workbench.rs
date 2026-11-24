mod object_fields;
mod row_removal;
mod table_cells;
mod table_projection;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use tomato_core::workbench::*;
pub fn session_database<'a>(connection: &'a tomato_core::Connection, schema: &'a str) -> &'a str {
    if matches!(connection.kind(), "MYSQL" | "HIVE") && !schema.is_empty() {
        schema
    } else if connection.kind() == "HIVE" {
        tomato_core::database_scope::hive_name(connection.text("database"))
    } else {
        connection.text("database")
    }
}
/// Display/file name of the selected database; SQLite's session key remains its file path.
pub fn query_database<'a>(connection: &'a tomato_core::Connection, schema: &'a str) -> &'a str {
    if matches!(connection.kind(), "SQLITE" | "ORACLE") {
        schema
    } else {
        session_database(connection, schema)
    }
}
pub fn query_connection(
    connection: &tomato_core::Connection,
    schema: &str,
) -> tomato_core::Connection {
    if matches!(connection.kind(), "MYSQL" | "HIVE" | "ORACLE") {
        tomato_core::database_scope::select(connection, schema)
    } else {
        connection.clone()
    }
}

#[derive(Default)]
pub struct Workbench {
    pub failure: Option<Failure>,
    pub session_lost: bool,
    pub browser: Option<Box<super::database_objects::window::Draft>>,
    pub designer: Option<Box<super::table_design::Draft>>,
    pub selectors: super::sql_scope::Selectors,
    pub editor_split: Option<f32>,
    pub result_context: Option<super::sql_scope::ResultContext>,
    pub ai: super::ai_sql::State,
    pub results_generation: u64,
    pub execution: Option<tomato_services::database::script::Mode>,
    pub script: Option<Box<tomato_services::database::script::Script>>,
    pub script_info: Option<String>,
    pub script_view: usize,
    pub query_results: Vec<super::query_results::State>,
    pub document: Option<super::queries::Document>,
    pub completion: super::completion::State,
    pub schemas: Arc<Vec<String>>,
    pub schema: String,
    pub tables: Arc<Vec<TableInfo>>,
    pub objects: Option<Arc<tomato_core::database_objects::Catalog>>,
    pub design_metadata: Option<Arc<tomato_core::table_design::Metadata>>,
    pub table: Option<TableId>,
    /// Known from the tree before metadata arrives, including failed loads.
    pub table_is_view: bool,
    pub(crate) object_load_pending: bool,
    pub definition: Option<TableDefinition>,
    pub page: Option<TablePage>,
    pub edits: BTreeMap<usize, Vec<Cell>>,
    pub deleted: BTreeSet<usize>,
    pub inserted: Vec<Vec<Option<Cell>>>,
    pub selected: BTreeSet<usize>,
    pub editor: Option<CellEditor>,
    pub batch: Option<Box<super::batch_edit::Batch>>,
    pub structure: bool,
    pub transaction: bool,
    pub sort: Option<Sort>,
    pub query: TableQuery,
    pub status: String,
    pub needs_refresh: bool,
    pub pending_page: Option<PageRequest>,
    pub confirm_delete: bool,
    pub page_size: u32,
    pub search: String,
    pub table_view: TableView,
}
pub struct Failure {
    pub text: String,
    pub focus: bool,
}
/// Transient viewport state; never serialized into the shared Java configuration.
#[derive(Default)]
pub struct TableView {
    pub query_draft: Option<TableQuery>,
    pub reset_scroll: bool,
    pub export_requested: bool,
    pub projection: table_projection::Projection,
    pub row_mode: bool,
    pub selector_focus: bool,
    pub selector_rows: BTreeSet<usize>,
    pub row_anchor: Option<usize>,
    pub row_drag: Option<usize>,
    pub paste_available: bool,
    pub pending_paste: bool,
    pub widths: Vec<f32>,
    pub cells: table_cells::Cells,
    pub anchor: Option<(usize, usize)>,
    pub focus: Option<(usize, usize)>,
    pub jump: String,
    pub jump_offset: Option<u64>,
    pub press: Option<CellPress>,
}
#[derive(Clone, Copy)]
pub struct CellPress {
    pub cell: (usize, usize),
    pub selected: bool,
}
pub struct CellEditor {
    pub row: usize,
    pub column: usize,
    pub text: String,
    pub null: bool,
    pub default: bool,
    pub binary: bool,
    pub changed: bool,
    pub focus: bool,
    pub select_all: bool,
    pub rect: Option<egui::Rect>,
    pub input: super::text_state::Editor,
    pub temporal: Option<Box<super::query_results::temporal::Picker>>,
    pub context: Option<egui::Context>,
    pub id: Option<egui::Id>,
}
impl CellEditor {
    pub fn new(row: usize, column: usize, value: Option<&Cell>, binary: bool) -> Self {
        Self {
            row,
            column,
            text: match value {
                None | Some(Cell::Null) => String::new(),
                Some(value) => value.display(),
            },
            null: value == Some(&Cell::Null),
            default: value.is_none(),
            binary,
            changed: false,
            focus: true,
            select_all: true,
            rect: None,
            input: Default::default(),
            temporal: None,
            context: None,
            id: None,
        }
    }
}
impl Drop for CellEditor {
    fn drop(&mut self) {
        if let Some(ctx) = &self.context {
            self.input.release(ctx);
            if let Some(picker) = &self.temporal {
                picker.release(ctx);
                if let Some(id) = self.id {
                    ctx.send_viewport_cmd_to(
                        egui::ViewportId::from_hash_of((id, "temporal-picker")),
                        egui::ViewportCommand::Close,
                    );
                }
            }
        }
    }
}
impl Workbench {
    pub fn dirty(&self) -> bool {
        !self.edits.is_empty()
            || !self.deleted.is_empty()
            || !self.inserted.is_empty()
            || self.editor.as_ref().is_some_and(|editor| editor.changed)
            || self.batch.is_some()
            || self.query_results.iter().any(|state| {
                state
                    .editing
                    .as_ref()
                    .is_some_and(|editing| editing.dirty())
            })
    }
    pub fn clear_changes(&mut self) {
        self.batch = None;
        self.edits.clear();
        self.deleted.clear();
        self.inserted.clear();
        self.selected.clear();
        self.table_view.cells.clear();
        self.table_view.anchor = None;
        self.table_view.focus = None;
        self.table_view.press = None;
        self.table_view.row_drag = None;
        self.table_view.row_mode = false;
        self.table_view.selector_focus = false;
        self.table_view.selector_rows.clear();
        self.table_view.row_anchor = None;
        self.table_view.pending_paste = false;
        self.editor = None;
    }
    pub fn request(&self) -> Option<PageRequest> {
        Some(PageRequest {
            table: self.table.clone()?,
            offset: self.page.as_ref().map_or(0, |page| page.offset),
            limit: if self.page_size == 0 {
                1000
            } else {
                self.page_size
            },
            sort: self.sort.clone(),
            query: self.query.clone(),
        })
    }
    /// Queue a server query without changing the page/selection before success.
    /// The existing discard dialog protects pending row edits.
    pub fn queue_query(&mut self, query: TableQuery) {
        if let Some(mut request) = self.request() {
            request.sort = None;
            request.query = query;
            request.offset = 0;
            self.pending_page = Some(request);
        }
    }
    fn receive_page(&mut self, page: TablePage) {
        self.table_view.reset_scroll |= self
            .page
            .as_ref()
            .is_none_or(|old| old.offset != page.offset || old.sql != page.sql);
        self.clear_changes();
        self.sort = None;
        self.table_view.query_draft = None;
        self.query = page.query.clone();
        self.search = page.query.search.clone();
        let index = |name: &str| {
            page.definition
                .columns
                .iter()
                .position(|column| column.name == name)
        };
        self.table_view.projection.sorts = page
            .query
            .sorts
            .iter()
            .filter_map(|sort| index(&sort.column).map(|i| (i, sort.descending)))
            .collect();
        self.table_view.projection.filters = page
            .query
            .filters
            .iter()
            .filter_map(|filter| index(&filter.column).map(|i| (i, filter.condition.clone())))
            .collect();
        self.table_view.projection.order.clear();
        self.definition = Some(page.definition.clone());
        self.page = Some(page);
        self.needs_refresh = false;
        self.session_lost = false;
    }
    pub fn changes(&self) -> Vec<RowChange> {
        let Some(page) = &self.page else {
            return Vec::new();
        };
        let mut changes = Vec::new();
        for &index in &self.deleted {
            changes.push(RowChange::Delete {
                original: page.rows[index].clone(),
            });
        }
        for (&index, values) in &self.edits {
            if !self.deleted.contains(&index) {
                changes.push(RowChange::Update {
                    original: page.rows[index].clone(),
                    values: values.clone(),
                });
            }
        }
        changes.extend(
            self.inserted
                .iter()
                .cloned()
                .map(|values| RowChange::Insert { values }),
        );
        changes
    }
    pub(crate) fn record_object_report(
        &mut self,
        report: &tomato_core::database_objects::ChangeReport,
    ) {
        self.status = format!(
            "成功 {} 个，失败 {} 个，影响 {} 行",
            report.succeeded.len(),
            report.failures.len(),
            report.affected
        );
        for (table, error) in &report.failures {
            self.status.push_str(&format!("\n{}: {error}", table.name));
        }
    }
    pub fn receive(&mut self, output: DatabaseOutput) {
        match output {
            DatabaseOutput::DesignSql { table, sql } => {
                if let Some(designer) = &mut self.designer {
                    designer.receive_sql(&table, sql);
                }
            }
            DatabaseOutput::Design(snapshot) => {
                if let Some(designer) = &mut self.designer {
                    // The controller releases active text state before dispatching a refresh/save.
                    designer.load(snapshot);
                    designer.busy = designer.metadata.is_none();
                    if designer.created {
                        let text = format!("表 {} 创建成功", designer.request.table.name);
                        designer.status = text.clone();
                        designer.notice = Some(super::table_design::Notice {
                            title: "新建表".into(),
                            header: None,
                            text,
                            error: false,
                            focus: true,
                        });
                        designer.created = false;
                    }
                }
            }
            DatabaseOutput::DesignInfo { page, mut info } => {
                if let Some(draft) = &mut self.designer
                    && let Some(saved) = &mut draft.saved
                {
                    use tomato_core::table_design::alter::InfoPage;
                    let index = match page {
                        InfoPage::Indexes => 0,
                        InfoPage::ForeignKeys => 1,
                        InfoPage::Triggers => 2,
                    };
                    if matches!(page, InfoPage::Indexes) {
                        let indexes = std::mem::take(&mut info.index_definitions);
                        draft.request.indexes = indexes.clone();
                        saved.definition.indexes = indexes;
                        draft.indexes = Default::default();
                        draft.revision = draft.revision.wrapping_add(1);
                    }
                    draft.info[index].reset();
                    draft.info_loaded[index] = true;
                    let (target, label) = match page {
                        InfoPage::Indexes => (&mut saved.indexes, "索引"),
                        InfoPage::ForeignKeys => (&mut saved.foreign_keys, "外键"),
                        InfoPage::Triggers => (&mut saved.triggers, "触发器"),
                    };
                    draft.status = format!("共 {} 个{label}", info.rows.len());
                    *target = info;
                }
            }
            DatabaseOutput::DesignMetadata(metadata) => {
                let metadata = Arc::new(metadata);
                if let Some(designer) = &mut self.designer {
                    designer.receive_metadata(metadata.clone());
                    designer.busy = false;
                }
                self.design_metadata = Some(metadata)
            }
            DatabaseOutput::TableCreated(table) => {
                self.status = format!("表 {} 创建成功", table.name);
            }
            DatabaseOutput::Objects(catalog) => {
                self.receive_object_catalog(catalog, false);
            }
            DatabaseOutput::ObjectHeaders(catalog) => self.receive_object_catalog(catalog, true),
            DatabaseOutput::ObjectFields { schema, fields } => {
                if schema == self.schema
                    && self.receive_object_fields(fields).is_err()
                    && let Some(browser) = &mut self.browser
                {
                    browser.fail_fields();
                }
            }
            DatabaseOutput::ObjectsChanged(report) => self.record_object_report(&report),
            DatabaseOutput::Completion(metadata) => {
                if metadata.schema == self.schema {
                    self.completion.metadata = Some(std::sync::Arc::new(metadata));
                    self.completion.error.clear();
                    self.completion.catalog_changed = true;
                }
            }
            DatabaseOutput::Databases(databases) => self.selectors.databases = Arc::new(databases),
            DatabaseOutput::Schemas(schemas) => self.schemas = Arc::new(schemas),
            DatabaseOutput::Tables(tables) => self.tables = Arc::new(tables),
            DatabaseOutput::Definition(definition) => self.definition = Some(definition),
            DatabaseOutput::Page(page) => {
                self.receive_page(page);
                self.status.clear();
            }
            DatabaseOutput::Deleted {
                table,
                rows,
                affected,
            } => self.remove_rows(&table, rows, affected),
            DatabaseOutput::Applied {
                affected,
                page,
                refresh_error,
            } => {
                self.clear_changes();
                self.needs_refresh = page.is_none();
                if let Some(page) = page {
                    self.receive_page(page);
                }
                self.status = refresh_error.unwrap_or_else(|| format!("已保存 {affected} 行修改"));
            }
            DatabaseOutput::Transaction(active) => self.transaction = active,
        }
    }
}
