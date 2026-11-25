//! One bounded index selection and one transient editor per designer.
use super::*;
use tomato_core::table_design::indexes::{Field, Index, Kind, methods};

pub struct Editor {
    pub cell: Cell,
    pub original: String,
    pub focus: bool,
}
pub struct Fields {
    pub popup: Option<egui::Rect>,
    pub child: Option<egui::Id>,
    pub anchor: Option<egui::Rect>,
    pub open: bool,
    pub original: Vec<Field>,
    pub row: usize,
    pub entries: Vec<(bool, Field)>,
    pub focus: Option<usize>,
    pub error: String,
}
pub struct State {
    pub widths: [f32; 6],
    pub selected: Vec<bool>,
    pub anchor: Option<usize>,
    pub editor: Option<Editor>,
    pub text: super::super::text_state::Editor,
    pub fields: Option<Fields>,
    pub drag: Option<usize>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            widths: [22., 180., 260., 120., 120., 200.],
            selected: Vec::new(),
            anchor: None,
            editor: None,
            text: Default::default(),
            fields: None,
            drag: None,
        }
    }
}
impl Draft {
    pub fn index_changes(&self) -> bool {
        self.saved
            .as_ref()
            .is_some_and(|s| s.definition.indexes != self.request.indexes)
    }
    pub fn select_index(&mut self, row: usize, modifiers: egui::Modifiers) {
        self.indexes
            .selected
            .resize(self.request.indexes.len(), false);
        if row >= self.indexes.selected.len() {
            return;
        }
        if modifiers.shift {
            if !modifiers.command {
                self.indexes.selected.fill(false);
            }
            let anchor = self.indexes.drag.or(self.indexes.anchor).unwrap_or(row);
            self.indexes.selected[anchor.min(row)..=anchor.max(row)].fill(true);
        } else {
            if !modifiers.command {
                self.indexes.selected.fill(false);
            }
            self.indexes.selected[row] = !modifiers.command || !self.indexes.selected[row];
            self.indexes.anchor = Some(row);
        }
    }
    pub fn add_index(&mut self) {
        if self.request.indexes.len() >= 1024 {
            self.status = "索引超过 1,024 项".into();
            return;
        }
        let row = self.request.indexes.len();
        let mut n = row + 1;
        while self
            .request
            .indexes
            .iter()
            .any(|i| i.name == format!("idx_{n}"))
        {
            n += 1;
        }
        self.request.indexes.push(Index {
            name: format!("idx_{n}"),
            kind: Kind::Normal,
            method: methods(self.dialect, Kind::Normal)
                .first()
                .unwrap_or(&"")
                .to_string(),
            ..Default::default()
        });
        self.select_index(row, egui::Modifiers::NONE);
        self.edit_index(Cell { row, column: 1 });
        self.changed();
        self.status = "已添加索引，请选择字段（未保存）".into();
    }
    pub fn delete_indexes(&mut self) {
        let selected = &self.indexes.selected;
        if !selected.iter().any(|b| *b) {
            self.status = "请先选择要删除的索引".into();
            return;
        }
        if self
            .request
            .indexes
            .iter()
            .enumerate()
            .any(|(row, i)| selected.get(row) == Some(&true) && i.constraint)
        {
            self.status = "所选索引由约束管理，请通过约束 SQL 修改".into();
            return;
        }
        let mut row = 0;
        self.request.indexes.retain(|_| {
            let keep = selected.get(row) != Some(&true);
            row += 1;
            keep
        });
        self.indexes.selected.clear();
        self.indexes.editor = None;
        self.indexes.fields = None;
        self.indexes.drag = None;
        self.changed();
        self.status = "已删除所选索引（未保存）".into();
    }
    pub fn edit_index(&mut self, cell: Cell) {
        let Some(index) = self.request.indexes.get(cell.row) else {
            return;
        };
        if index.constraint && cell.column != 1 {
            self.status = "此索引由约束管理，请通过约束 SQL 修改".into();
            return;
        }
        if cell.column != 1 && (!index.opaque.is_empty() || matches!(self.dialect, Dialect::Oracle))
        {
            self.status = "此索引属性请通过 SQL 修改".into();
            return;
        }
        self.indexes.editor = Some(Editor {
            cell,
            original: if cell.column == 1 {
                index.name.clone()
            } else {
                index.comment.clone()
            },
            focus: true,
        });
    }
    pub fn index_fields(&mut self, row: usize) {
        let Some(index) = self.request.indexes.get(row) else {
            return;
        };
        if index.constraint || !index.opaque.is_empty() {
            self.status = "此索引含受保护属性，请通过 SQL 修改".into();
            return;
        }
        self.indexes.editor = None;
        let mut entries: Vec<_> = index.fields.iter().cloned().map(|f| (true, f)).collect();
        for column in &self.request.columns {
            if !column.name.is_empty() && !entries.iter().any(|(_, f)| f.name == column.name) {
                entries.push((
                    false,
                    Field {
                        name: column.name.clone(),
                        ..Default::default()
                    },
                ));
            }
        }
        self.indexes.fields = Some(Fields {
            popup: None,
            child: None,
            anchor: None,
            open: true,
            original: index.fields.clone(),
            row,
            entries,
            focus: None,
            error: String::new(),
        });
    }
}

impl State {
    pub fn close_fields(&mut self, ctx: &egui::Context) {
        if let Some(fields) = self.fields.take()
            && let Some(child) = fields.child
        {
            egui::Popup::close_id(ctx, child);
        }
    }
}
