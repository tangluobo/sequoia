//! Data export contracts shared by the wizard and streaming writers.
use crate::workbench::{Cell, Column, TableId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Dbase,
    #[default]
    Text,
    Csv,
    Html,
    Xls,
    Xlsx,
    Sql,
    Xml,
    Json,
    Access,
}
impl Format {
    pub const ALL: [Self; 10] = [
        Self::Dbase,
        Self::Text,
        Self::Csv,
        Self::Html,
        Self::Xls,
        Self::Xlsx,
        Self::Sql,
        Self::Xml,
        Self::Json,
        Self::Access,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Dbase => "DBase 文件 (*.dbf)",
            Self::Text => "文本文件 (*.txt)",
            Self::Csv => "CSV 文件 (*.csv)",
            Self::Html => "HTML 文件 (*.htm;*.html)",
            Self::Xls => "Excel 数据表 (*.xls)",
            Self::Xlsx => "Excel 文件 (2007 或更高版本) (*.xlsx)",
            Self::Sql => "SQL 脚本文件 (*.sql)",
            Self::Xml => "XML 文件 (*.xml)",
            Self::Json => "JSON 文件 (*.json)",
            Self::Access => "MS Access 2007 数据库 (*.accdb)",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Self::Dbase => "dbf",
            Self::Text => "txt",
            Self::Csv => "csv",
            Self::Html => "html",
            Self::Xls => "xls",
            Self::Xlsx => "xlsx",
            Self::Sql => "sql",
            Self::Xml => "xml",
            Self::Json => "json",
            Self::Access => "accdb",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Schema {
    pub table: TableId,
    pub dialect: String,
    pub columns: Vec<Column>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    pub format: Format,
    pub fields: Vec<usize>,
    pub field_names: Vec<String>,
    pub expected_columns: Vec<String>,
    pub header: bool,
    pub delimiter: char,
    pub bom: bool,
    pub null_text: String,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            format: Format::Text,
            fields: Vec::new(),
            field_names: Vec::new(),
            expected_columns: Vec::new(),
            header: true,
            delimiter: '\t',
            bom: true,
            null_text: "\\N".into(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub schema: Schema,
    pub rows: Vec<Vec<Cell>>,
}
