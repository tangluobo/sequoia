//! Table designer state: one column draft, compact cell selection and one active editor.
use std::sync::Arc;
use tomato_core::table_design::{Column, Create, Dialect, Metadata, Options, data_types};
use tomato_core::workbench::TableId;
mod clipboard;
mod flags;
mod selection;
pub use selection::Drag;
pub mod indexes;
pub mod info;
pub mod preview;
pub mod type_picker;
use flags::Flags;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Fields,
    Indexes,
    ForeignKeys,
    Triggers,
    Options,
    Comment,
    Sql,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub row: usize,
    pub column: usize,
}
#[derive(Clone, Copy)]
pub enum Action {
    Add,
    Insert,
    Delete,
    PrimaryKey,
    Up,
    Down,
}
pub struct Edit {
    pub cell: Cell,
    pub text: String,
    pub editor: super::text_state::Editor,
    pub focus: bool,
    pub type_picker: Option<Box<type_picker::Picker>>,
}
pub struct Notice {
    pub title: String,
    pub header: Option<String>,
    pub text: String,
    pub error: bool,
    pub focus: bool,
}
pub struct Name {
    pub text: String,
    pub focus: bool,
    pub editor: super::text_state::Editor,
}
#[derive(Default)]
pub enum SqlDefinition {
    #[default]
    Unloaded,
    Loading,
    Ready,
    Failed(String),
}
pub struct Draft {
    pub saved: Option<Box<tomato_core::table_design::alter::Snapshot>>,
    pub origins: Vec<Option<String>>,
    pub widths: [f32; 9],
    pub info: [info::State; 3],
    pub indexes: indexes::State,
    pub info_loaded: [bool; 3],
    text_ids: Vec<egui::Id>,
    pub split: Option<f32>,
    pub scroll_to: Option<usize>,
    pub drag: Option<Drag>,
    pub name: Option<Name>,
    pub notice: Option<Notice>,
    pub saving: bool,
    pub created: bool,
    pub request: Create,
    pub dialect: Dialect,
    pub metadata: Option<Arc<Metadata>>,
    pub page: Page,
    pub focus: Option<Cell>,
    pub anchor: Option<Cell>,
    selected: Vec<u16>,
    /// Distinguishes the original appended-row empty flags from explicit "否".
    flags: Vec<Flags>,
    pub edit: Option<Edit>,
    pub dirty: bool,
    pub busy: bool,
    pub options_loaded: bool,
    pub comment_loaded: bool,
    pub revision: u64,
    pub status: String,
    pub preview: String,
    pub preview_save_as: bool,
    pub sql_definition: SqlDefinition,
    pub sql_generation: u64,
    pub preview_revision: Option<u64>,
    pub preview_view: preview::Viewer,
    pub preview_editor: super::text_state::Editor,
    pub copied_fields: Option<String>,
}
impl Draft {
    pub fn new(kind: &str, schema: String) -> anyhow::Result<Self> {
        Ok(Self {
            saved: None,
            origins: vec![None],
            indexes: Default::default(),
            info: std::array::from_fn(|_| info::State::default()),
            info_loaded: [false; 3],
            text_ids: Vec::new(),
            widths: [15., 150., 120., 60., 60., 60., 70., 120., 200.],
            split: None,
            scroll_to: None,
            drag: None,
            name: None,
            notice: None,
            saving: false,
            created: false,
            request: Create {
                auxiliary: Vec::new(),
                indexes: Vec::new(),
                table: TableId {
                    schema,
                    name: String::new(),
                },
                columns: vec![Column::default()],
                options: Options::default(),
                comment: String::new(),
            },
            dialect: Dialect::resolve(kind)?,
            metadata: None,
            page: Page::Fields,
            focus: None,
            anchor: None,
            selected: Vec::new(),
            flags: vec![Flags::default()],
            edit: None,
            dirty: false,
            busy: false,
            options_loaded: false,
            comment_loaded: false,
            revision: 0,
            status: "新建表".into(),
            preview: String::new(),
            preview_save_as: false,
            sql_definition: SqlDefinition::Unloaded,
            sql_generation: 0,
            preview_revision: None,
            preview_view: Default::default(),
            preview_editor: Default::default(),
            copied_fields: None,
        })
    }
    pub fn load(&mut self, snapshot: Box<tomato_core::table_design::alter::Snapshot>) {
        debug_assert!(self.edit.is_none());
        self.sql_generation = self.sql_generation.wrapping_add(1);
        self.clear_selection();
        for state in &mut self.info {
            state.reset();
        }
        self.info_loaded = [
            true,
            !snapshot.foreign_keys.headers.is_empty(),
            !snapshot.triggers.headers.is_empty(),
        ];
        self.indexes = Default::default();
        self.origins = snapshot
            .definition
            .columns
            .iter()
            .map(|c| Some(c.name.clone()))
            .collect();
        self.request = snapshot.definition.clone();
        self.flags = vec![Flags::default(); self.request.columns.len()];
        self.sql_definition = if snapshot.sql.is_empty() {
            SqlDefinition::Unloaded
        } else {
            SqlDefinition::Ready
        };
        self.saved = Some(snapshot);
        self.dirty = false;
        self.options_loaded = false;
        self.comment_loaded = false;
        self.revision = self.revision.wrapping_add(1);
        self.preview_revision = None;
        self.preview_view.key = None;
        self.status = if self.saving {
            "变更已保存".into()
        } else {
            format!("共 {} 个字段", self.request.columns.len())
        };
        self.saving = false;
    }
    pub fn set_flag(&mut self, row: usize, bit: usize, value: bool) {
        let Some(column) = self.request.columns.get_mut(row) else {
            return;
        };
        match bit {
            0 => column.not_null = value,
            1 => column.primary_key = value,
            2 => {
                column.auto_increment = value;
                if matches!(self.dialect, Dialect::Postgres | Dialect::Oracle) {
                    if value {
                        column.default_value.clear();
                        column.default_expression = false;
                        column.not_null = true;
                        self.flags[row].set(0, true);
                    } else if column.generation == tomato_core::table_design::Generation::Serial {
                        column.default_value.clear();
                        column.default_expression = false;
                    }
                }
            }
            3 => column.unsigned = value,
            4 => column.zerofill = value,
            5 => column.binary = value,
            _ => return,
        }
        self.flags[row].set(bit, value);
        self.changed();
    }
    pub fn edit_request(&self) -> Option<tomato_core::table_design::alter::Edit> {
        Some(tomato_core::table_design::alter::Edit {
            before: self.saved.as_ref()?.baseline(),
            after: self.snapshot(),
            origins: self.origins.clone(),
        })
    }
    pub fn receive_metadata(&mut self, metadata: Arc<Metadata>) {
        self.preview_revision = None;
        if self.saved.is_none() {
            self.status = metadata.version.as_ref().map_or_else(
                || "新建表".into(),
                |version| format!("新建表 | 版本: {version}"),
            );
        }
        self.metadata = Some(metadata);
        if self.page == Page::Options && !self.options_loaded {
            self.initialize_options();
        }
    }
    pub fn types(&self) -> &'static [&'static str] {
        self.metadata.as_ref().map_or_else(
            || data_types(self.dialect, None),
            |metadata| metadata.data_types,
        )
    }
    pub fn changed(&mut self) {
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn trim_text(&self, ctx: &egui::Context) {
        for id in &self.text_ids {
            if let Some(state) = egui::widgets::text_edit::TextEditState::load(ctx, *id) {
                state.trim_undo_memory(2 * 1024 * 1024);
            }
        }
    }
    pub fn own_text(&mut self, id: egui::Id) {
        if !self.text_ids.contains(&id) {
            self.text_ids.push(id);
        }
    }
    pub fn has_changes(&self) -> bool {
        self.dirty
            || self.edit.as_ref().is_some_and(|edit| {
                self.request
                    .columns
                    .get(edit.cell.row)
                    .is_some_and(|column| field(column, edit.cell.column) != edit.text)
            })
    }
    pub fn begin_edit(&mut self, ctx: &egui::Context, cell: Cell) {
        if cell.row >= self.request.columns.len() || ![0, 1, 2, 3, 7].contains(&cell.column) {
            return;
        }
        self.commit_edit(ctx);
        self.drag = None;
        self.select(cell, false, false);
        self.edit = Some(Edit {
            cell,
            text: field(&self.request.columns[cell.row], cell.column).into(),
            editor: Default::default(),
            focus: true,
            type_picker: (cell.column == 1).then(|| {
                Box::new(type_picker::Picker::new(
                    self.types(),
                    &self.request.columns[cell.row].data_type,
                ))
            }),
        });
    }
    pub fn commit_edit(&mut self, ctx: &egui::Context) {
        self.indexes.text.release(ctx);
        self.indexes.close_fields(ctx);
        self.indexes.editor = None;
        if let Some(edit) = self.edit.take() {
            if let Some(picker) = edit.type_picker {
                picker.release(ctx);
            }
            edit.editor.release(ctx);
            self.set_field(edit.cell, edit.text);
        }
    }
    pub fn cancel_edit(&mut self, ctx: &egui::Context) {
        self.indexes.text.release(ctx);
        self.indexes.close_fields(ctx);
        self.indexes.editor = None;
        if let Some(edit) = self.edit.take() {
            if let Some(picker) = edit.type_picker {
                picker.release(ctx);
            }
            edit.editor.release(ctx);
        }
    }
    pub fn normalize_default(&mut self, row: usize) {
        let original = self.saved.as_ref().and_then(|saved| {
            self.origins
                .get(row)
                .and_then(|name| name.as_ref())
                .and_then(|name| {
                    saved
                        .definition
                        .columns
                        .iter()
                        .find(|column| &column.name == name)
                })
        });
        if let Some(column) = self.request.columns.get_mut(row) {
            column.default_expression = original.is_some_and(|old| {
                old.default_value == column.default_value && old.default_expression
            });
        }
    }
    pub fn set_field(&mut self, cell: Cell, text: String) {
        let types = self.types();
        let Some(column) = self.request.columns.get_mut(cell.row) else {
            return;
        };
        let slot = match cell.column {
            0 => &mut column.name,
            1 => &mut column.data_type,
            2 => &mut column.length,
            3 => &mut column.scale,
            6 => &mut column.default_value,
            7 => &mut column.comment,
            _ => return,
        };
        let previous_name = (cell.column == 0).then(|| slot.clone());
        let changed = *slot != text;
        *slot = text;
        let inferred = cell.column == 0 && column.infer_if_unset(self.dialect, types);
        if let Some(previous) = previous_name {
            let name = column.name.clone();
            for index in &mut self.request.indexes {
                for f in &mut index.fields {
                    if f.name == previous {
                        f.name = name.clone();
                    }
                }
            }
        }
        if cell.column == 6 {
            self.normalize_default(cell.row);
        }
        if changed || inferred {
            self.changed();
        }
    }
    pub fn append(&mut self) -> Option<usize> {
        if self.request.columns.len() >= 4096 {
            self.status = "字段超过 4,096 项".into();
            return None;
        }
        let row = self.request.columns.len();
        self.request.columns.push(Column::default());
        self.flags.push(Flags::empty());
        self.origins.push(None);
        self.changed();
        self.status = "已添加字段行（未保存）".into();
        Some(row)
    }
    pub fn perform(&mut self, ctx: &egui::Context, action: Action) {
        if self.busy {
            return;
        }
        self.commit_edit(ctx);
        match action {
            Action::Add => self.add(ctx),
            Action::Insert => self.insert(ctx),
            Action::Delete => self.delete(),
            Action::PrimaryKey => self.toggle_primary_key(),
            Action::Up => self.move_row(false),
            Action::Down => self.move_row(true),
        }
    }
    fn add(&mut self, ctx: &egui::Context) {
        if let Some(row) = self.append() {
            self.scroll_to = Some(row);
            self.begin_edit(ctx, Cell { row, column: 0 });
        }
    }
    fn insert(&mut self, ctx: &egui::Context) {
        if self.request.columns.is_empty() {
            self.add(ctx);
            return;
        }
        if self.request.columns.len() >= 4096 {
            self.status = "字段超过 4,096 项".into();
            return;
        }
        let row = self
            .focus
            .map_or(self.request.columns.len(), |cell| cell.row);
        self.request.columns.insert(row, Column::default());
        self.flags.insert(row, Flags::empty());
        self.origins.insert(row, None);
        self.changed();
        self.status = "已插入字段行（未保存）".into();
        self.scroll_to = Some(row);
        self.begin_edit(ctx, Cell { row, column: 0 });
    }
    fn delete(&mut self) {
        let rows = self.selected_rows();
        if rows.is_empty() {
            self.status = if self.request.columns.is_empty() {
                "请先加载表结构"
            } else {
                "请先选择要删除的字段"
            }
            .into();
            return;
        }
        // Keep each survivor once instead of shifting the remaining columns per deletion.
        let selected = &self.selected;
        let mut row = 0;
        self.request.columns.retain(|_| {
            let keep = selected.get(row).is_none_or(|mask| *mask == 0);
            row += 1;
            keep
        });
        row = 0;
        self.flags.retain(|_| {
            let keep = selected.get(row).is_none_or(|mask| *mask == 0);
            row += 1;
            keep
        });
        row = 0;
        self.origins.retain(|_| {
            let keep = selected.get(row).is_none_or(|mask| *mask == 0);
            row += 1;
            keep
        });
        self.clear_selection();
        self.changed();
        self.status = format!("已删除 {} 个字段（未保存）", rows.len());
    }
    fn toggle_primary_key(&mut self) {
        let Some(row) = self.focus.map(|cell| cell.row) else {
            self.status = "请先选择一个字段".into();
            return;
        };
        let Some(column) = self.request.columns.get_mut(row) else {
            return;
        };
        column.primary_key = !column.primary_key;
        self.flags[row].set(1, column.primary_key);
        self.changed();
        self.status = "已切换主键（未保存）".into();
    }
    fn move_row(&mut self, down: bool) {
        let Some(row) = self.focus.map(|cell| cell.row) else {
            self.status = "请先选择一个字段".into();
            return;
        };
        if !down && row == 0 {
            self.status = "已在顶部".into();
            return;
        }
        if down && row + 1 >= self.request.columns.len() {
            self.status = "已在底部".into();
            return;
        }
        let next = if down { row + 1 } else { row - 1 };
        self.request.columns.swap(row, next);
        self.flags.swap(row, next);
        self.origins.swap(row, next);
        self.select_row(next);
        self.changed();
        self.status = if down {
            "已下移字段（未保存）"
        } else {
            "已上移字段（未保存）"
        }
        .into();
    }
    pub fn switch_page(&mut self, ctx: &egui::Context, page: Page) {
        self.commit_edit(ctx);
        self.indexes.editor = None;
        self.indexes.fields = None;
        self.drag = None;
        if page == Page::Sql
            && self
                .saved
                .as_ref()
                .is_some_and(|saved| saved.view || self.preview_save_as)
        {
            self.invalidate_sql();
        }
        self.page = page;
        if matches!(page, Page::Indexes | Page::ForeignKeys | Page::Triggers) {
            let (count, label) = match page {
                Page::Indexes => (self.request.indexes.len(), "索引"),
                Page::ForeignKeys => (
                    self.info[1].rows.as_ref().map_or_else(
                        || self.saved.as_ref().map_or(0, |s| s.foreign_keys.rows.len()),
                        Vec::len,
                    ),
                    "外键",
                ),
                _ => (
                    self.info[2].rows.as_ref().map_or_else(
                        || self.saved.as_ref().map_or(0, |s| s.triggers.rows.len()),
                        Vec::len,
                    ),
                    "触发器",
                ),
            };
            self.status = format!("共 {count} 个{label}");
        }
        if page == Page::Options && !self.options_loaded && self.metadata.is_some() {
            self.initialize_options();
        }
        if page == Page::Comment && !self.comment_loaded {
            self.comment_loaded = true;
            self.status = if self.saved.is_some() {
                "表注释已加载"
            } else {
                "新建表注释"
            }
            .into();
        }
    }
    fn initialize_options(&mut self) {
        if self.saved.is_some() {
            self.options_loaded = true;
            self.status = "表选项已加载".into();
            return;
        }
        if let Some(metadata) = &self.metadata {
            self.request.options.engine = metadata
                .engines
                .iter()
                .find(|engine| engine.as_str() == "InnoDB")
                .or_else(|| metadata.engines.first())
                .cloned()
                .unwrap_or_default();
            if let Some(charset) = metadata
                .charsets
                .iter()
                .find(|charset| charset.name == "utf8mb4")
            {
                self.request.options.charset = charset.name.clone();
                self.request.options.collation =
                    charset.collations.first().cloned().unwrap_or_default();
            }
        }
        self.options_loaded = true;
        self.revision = self.revision.wrapping_add(1);
        self.status = "新建表选项已加载".into();
    }
    pub fn snapshot(&self) -> Create {
        let mut request = self.request.clone();
        if !self.options_loaded {
            request.options = self
                .saved
                .as_ref()
                .map_or_else(Options::default, |saved| saved.definition.options.clone());
        }
        if !self.comment_loaded {
            request.comment = self
                .saved
                .as_ref()
                .map_or_else(String::new, |saved| saved.definition.comment.clone());
        }
        // Keep secondary-index columns aligned with field renames without rebuilding indexes.
        for index in &mut request.indexes {
            for f in &mut index.fields {
                if let Some((_, c)) = self
                    .origins
                    .iter()
                    .zip(&request.columns)
                    .find(|(o, _)| o.as_ref() == Some(&f.name))
                {
                    f.name = c.name.clone();
                }
            }
        }
        for (index, page) in [
            (1, tomato_core::table_design::alter::InfoPage::ForeignKeys),
            (2, tomato_core::table_design::alter::InfoPage::Triggers),
        ] {
            if let Some(rows) = &self.info[index].rows {
                let before = self
                    .saved
                    .as_ref()
                    .map(|saved| {
                        if index == 1 {
                            &saved.foreign_keys.rows
                        } else {
                            &saved.triggers.rows
                        }
                    })
                    .cloned()
                    .unwrap_or_default();
                if *rows != before {
                    request
                        .auxiliary
                        .push(tomato_core::table_design::auxiliary::Edit {
                            before_signature: self.saved.as_ref().map_or_else(
                                String::new,
                                |saved| {
                                    if index == 1 {
                                        saved.foreign_keys.signature.clone()
                                    } else {
                                        saved.triggers.signature.clone()
                                    }
                                },
                            ),
                            page,
                            before,
                            after: rows.clone(),
                        });
                }
            }
        }
        request
    }
    pub fn backslash_escapes(&self) -> bool {
        self.saved.as_ref().map_or_else(
            || self.metadata.as_ref().is_some_and(|m| m.backslash_escapes),
            |s| s.backslash_escapes,
        )
    }
    pub fn update_preview(&mut self) {
        if self.preview_revision == Some(self.revision) {
            return;
        }
        self.status = format!("共 {} 个字段", self.request.columns.len());
        if let Some(saved) = &self.saved
            && (saved.view || self.preview_save_as)
        {
            self.preview = match &self.sql_definition {
                SqlDefinition::Ready => saved.sql.clone(),
                SqlDefinition::Unloaded | SqlDefinition::Loading => "-- 加载中...".into(),
                SqlDefinition::Failed(error) => {
                    self.status = "SQL预览加载失败".into();
                    let mut text = String::from("-- 无法获取原始对象定义\n");
                    for line in error.lines() {
                        text.push_str("-- ");
                        text.push_str(line);
                        text.push('\n');
                    }
                    text
                }
            };
            self.preview_revision = Some(self.revision);
            return;
        }
        if self.saved.is_none() {
            let errors = self.request.validation_errors();
            if !errors.is_empty() {
                self.preview = format!(
                    "-- 字段设置不完整，请检查：\n{}",
                    errors
                        .iter()
                        .map(|error| format!("-- {error}\n"))
                        .collect::<String>()
                );
                self.preview_revision = Some(self.revision);
                return;
            }
        }
        let mut request = self.snapshot();
        if self.saved.is_none() {
            request.table.name = "新表名".into();
        }
        self.preview = if request
            .columns
            .iter()
            .all(|column| column.name.trim_matches(|ch: char| ch <= ' ').is_empty())
        {
            "-- 请先添加至少一个字段（字段名不能为空）".into()
        } else {
            if let Some(edit) = self.edit_request().filter(|_| !self.preview_save_as) {
                edit.plan(self.dialect, self.backslash_escapes())
                    .map(|p| {
                        if p.statements.is_empty() {
                            "-- 没有需要保存的变更".into()
                        } else {
                            tomato_core::table_design::preview::render(self.dialect, &p.statements)
                        }
                    })
                    .unwrap_or_else(|e| format!("-- {e}"))
            } else {
                request
                    .sql_for_session(self.dialect, self.backslash_escapes())
                    .unwrap_or_else(|error| format!("-- {error}"))
            }
        };
        self.preview_revision = Some(self.revision);
    }
    pub fn receive_sql(&mut self, table: &TableId, sql: String) {
        if !matches!(self.sql_definition, SqlDefinition::Loading) {
            return;
        }
        let Some(saved) = self
            .saved
            .as_mut()
            .filter(|saved| &saved.definition.table == table)
        else {
            return;
        };
        saved.sql = sql;
        self.sql_definition = SqlDefinition::Ready;
        self.preview_revision = None;
        self.preview_view.key = None;
    }
    pub fn invalidate_sql(&mut self) {
        if matches!(self.sql_definition, SqlDefinition::Loading) {
            return;
        }
        if let Some(saved) = &mut self.saved {
            saved.sql = String::new();
        }
        self.sql_generation = self.sql_generation.wrapping_add(1);
        self.sql_definition = SqlDefinition::Unloaded;
        self.preview_revision = None;
        self.preview_view.key = None;
    }
    pub fn fail_sql(&mut self, error: String) {
        if matches!(self.sql_definition, SqlDefinition::Loading) {
            self.sql_definition = SqlDefinition::Failed(error);
            self.preview_revision = None;
            self.preview_view.key = None;
        }
    }
    pub fn release(&self, ctx: &egui::Context) {
        self.indexes.text.release(ctx);
        if let Some(fields) = &self.indexes.fields
            && let Some(child) = fields.child
        {
            egui::Popup::close_id(ctx, child);
        }
        for id in &self.text_ids {
            ctx.data_mut(|data| data.remove::<egui::widgets::text_edit::TextEditState>(*id));
        }
        if let Some(edit) = &self.edit {
            if let Some(picker) = &edit.type_picker {
                picker.release(ctx);
            }
            edit.editor.release(ctx);
        }
        self.preview_editor.release(ctx);
        if let Some(name) = &self.name {
            name.editor.release(ctx);
        }
    }
}
pub fn field(column: &Column, index: usize) -> &str {
    match index {
        0 => &column.name,
        1 => &column.data_type,
        2 => &column.length,
        3 => &column.scale,
        6 => &column.default_value,
        7 => &column.comment,
        _ => "",
    }
}
#[cfg(test)]
mod tests;
