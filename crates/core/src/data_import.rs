//! File import contracts; parsers, database drivers and widgets depend on these types.
use crate::workbench::{Cell, TableDefinition, TableId};
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PREVIEW_ROWS: usize = 100;
pub const MAX_COLUMNS: usize = 16_384;
pub const MAX_ROW_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Dbase,
    Paradox,
    #[default]
    Text,
    Csv,
    Excel,
    Xml,
    Json,
    Access,
    Odbc,
}
impl Format {
    pub const ALL: [Self; 9] = [
        Self::Dbase,
        Self::Paradox,
        Self::Text,
        Self::Csv,
        Self::Excel,
        Self::Xml,
        Self::Json,
        Self::Access,
        Self::Odbc,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Dbase => "DBase 文件 (*.dbf)",
            Self::Paradox => "Paradox 文件 (*.db)",
            Self::Text => "文本文件 (*.txt)",
            Self::Csv => "CSV 文件 (*.csv)",
            Self::Excel => "Excel 文件 (*.xls; *.xlsx)",
            Self::Xml => "XML 文件 (*.xml)",
            Self::Json => "JSON 文件 (*.json)",
            Self::Access => "MS Access 数据库 (*.mdb; *.accdb)",
            Self::Odbc => "ODBC",
        }
    }
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Dbase => &["dbf"],
            Self::Paradox => &["db"],
            Self::Text => &["txt", "tsv", "dat"],
            Self::Csv => &["csv"],
            Self::Excel => &["xls", "xlsx"],
            Self::Xml => &["xml"],
            Self::Json => &["json", "jsonl", "ndjson"],
            Self::Access => &["mdb", "accdb"],
            Self::Odbc => &[],
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct OdbcTable {
    pub catalog: String,
    pub schema: String,
    pub name: String,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Odbc {
    pub use_32bit: bool,
    pub dsn: String,
    pub user: String,
    pub password: String,
    pub connection_string: String,
    pub query: String,
    pub tables: Vec<OdbcTable>,
}
impl std::fmt::Debug for Odbc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Odbc")
            .field("use_32bit", &self.use_32bit)
            .field("tables", &self.tables)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Source {
    pub format: Format,
    pub files: Vec<PathBuf>,
    pub encoding: String,
    pub delimiter: char,
    pub quote: Option<char>,
    pub header_row: usize,
    pub first_row: u64,
    pub last_row: Option<u64>,
    pub skip_empty: bool,
    pub trim: bool,
    pub null_text: Option<String>,
    pub json_path: String,
    pub xml_row: String,
    pub date_order: String,
    pub date_separator: String,
    pub tables: Vec<String>,
    pub access_password: String,
    pub odbc: Odbc,
}
impl std::fmt::Debug for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Source")
            .field("format", &self.format)
            .field("files", &self.files)
            .field("encoding", &self.encoding)
            .field("odbc", &self.odbc)
            .finish_non_exhaustive()
    }
}
impl Default for Source {
    fn default() -> Self {
        Self {
            format: Format::Text,
            files: Vec::new(),
            encoding: "UTF-8".into(),
            delimiter: '\t',
            quote: Some('"'),
            header_row: 1,
            first_row: 2,
            last_row: None,
            skip_empty: true,
            trim: false,
            null_text: Some("\\N".into()),
            json_path: String::new(),
            xml_row: String::new(),
            date_order: "YMD".into(),
            date_separator: "-".into(),
            tables: Vec::new(),
            access_password: String::new(),
            odbc: Odbc::default(),
        }
    }
}
impl Source {
    /// Runtime transport may contain credentials; a user-saved profile never does.
    pub fn without_secrets(&self) -> Self {
        let mut s = self.clone();
        s.access_password.clear();
        s.odbc.password.clear();
        s.odbc.connection_string.clear();
        s
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.files.len() <= 100, "一次最多选择 100 个源文件");
        ensure!(
            self.format == Format::Odbc || !self.files.is_empty(),
            "请选择源文件"
        );
        ensure!(
            !self.delimiter.is_control() || self.delimiter == '\t',
            "字段分隔符无效"
        );
        ensure!(
            self.delimiter.is_ascii()
                && self
                    .quote
                    .is_none_or(|q| q.is_ascii() && q != self.delimiter && q != '\n' && q != '\r'),
            "分隔符和文本限定符必须是不同的单字节字符"
        );
        ensure!(
            self.header_row <= 1_000_000 && self.first_row > 0,
            "字段名行和开始行无效"
        );
        ensure!(
            self.last_row.is_none_or(|n| n >= self.first_row),
            "结束行不能早于开始行"
        );
        ensure!(
            self.json_path.len() <= 4096 && self.xml_row.len() <= 4096,
            "源路径过长"
        );
        ensure!(
            matches!(self.date_order.as_str(), "YMD" | "DMY" | "MDY")
                && self.date_separator.len() <= 4,
            "日期格式无效"
        );
        ensure!(
            self.encoding.len() <= 128 && self.null_text.as_ref().is_none_or(|s| s.len() <= 4096),
            "编码或 NULL 标记过长"
        );
        ensure!(
            self.tables.len() <= 100 && self.odbc.tables.len() <= 100,
            "一次最多选择 100 张源表"
        );
        ensure!(
            self.tables.iter().all(|s| s.len() <= 4096)
                && self.files.iter().all(|p| p.as_os_str().len() <= 32768),
            "源名称过长"
        );
        ensure!(
            self.access_password.len() <= 65536
                && [
                    &self.odbc.dsn,
                    &self.odbc.user,
                    &self.odbc.password,
                    &self.odbc.connection_string,
                    &self.odbc.query
                ]
                .into_iter()
                .all(|s| s.len() <= 65536 && !s.contains('\0')),
            "数据源设置过长或包含无效字符"
        );
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataType {
    Text,
    Integer,
    Decimal { precision: u16, scale: u16 },
    Real,
    Boolean,
    Date,
    DateTime,
    Binary,
}
impl DataType {
    pub fn label(&self) -> String {
        match self {
            Self::Text => "文本".into(),
            Self::Integer => "整数".into(),
            Self::Decimal { precision, scale } => format!("数值 ({precision},{scale})"),
            Self::Real => "浮点数".into(),
            Self::Boolean => "布尔".into(),
            Self::Date => "日期".into(),
            Self::DateTime => "日期时间".into(),
            Self::Binary => "二进制".into(),
        }
    }
    pub fn sql(&self, kind: &str, key: bool) -> Result<String> {
        Ok(match (self, kind) {
            (Self::Text, "MYSQL") if key => "VARCHAR(255)".into(),
            (Self::Text, "ORACLE") if key => "VARCHAR2(1000)".into(),
            (Self::Text, "ORACLE") => "CLOB".into(),
            (Self::Text, _) => "TEXT".into(),
            (Self::Integer, "ORACLE") => "NUMBER(19)".into(),
            (Self::Integer, _) => "BIGINT".into(),
            (Self::Decimal { .. }, "SQLITE") => "TEXT".into(),
            (Self::Decimal { precision, scale }, _) => {
                ensure!(
                    *precision > 0 && *precision <= 38 && *scale <= *precision,
                    "数值精度应为 1–38，且小数位不能超过精度"
                );
                format!("DECIMAL({precision},{scale})")
            }
            (Self::Real, "MYSQL") => "DOUBLE".into(),
            (Self::Real, "POSTGRESQL") => "DOUBLE PRECISION".into(),
            (Self::Real, "ORACLE") => "BINARY_DOUBLE".into(),
            (Self::Real, _) => "REAL".into(),
            (Self::Boolean, "ORACLE") => "NUMBER(1)".into(),
            (Self::Boolean, _) => "BOOLEAN".into(),
            (Self::Date, _) => "DATE".into(),
            (Self::DateTime, "MYSQL") => "DATETIME(6)".into(),
            (Self::DateTime, _) => "TIMESTAMP".into(),
            (Self::Binary, "POSTGRESQL") => "BYTEA".into(),
            (Self::Binary, _) => "BLOB".into(),
        })
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub data_type: DataType,
    pub nullable: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceTable {
    pub name: String,
    pub origin: String,
    pub fields: Vec<Field>,
    pub rows: u64,
    pub preview: Vec<Vec<Cell>>,
    pub spool: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mapping {
    pub enabled: bool,
    pub source: Option<usize>,
    #[serde(default)]
    pub source_name: Option<String>,
    pub default: Option<String>,
    pub target: String,
    pub data_type: DataType,
    pub nullable: bool,
    pub key: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub selected: bool,
    pub source: usize,
    #[serde(default)]
    pub source_key: Option<(String, String)>,
    pub table: TableId,
    pub create: bool,
    pub fields: Vec<Mapping>,
    pub expected: Option<TableDefinition>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Append,
    Update,
    AppendUpdate,
    SkipDuplicates,
    Delete,
    Copy,
}
impl Mode {
    pub const ALL: [Self; 6] = [
        Self::Append,
        Self::Update,
        Self::AppendUpdate,
        Self::SkipDuplicates,
        Self::Delete,
        Self::Copy,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Append => "追加",
            Self::Update => "更新",
            Self::AppendUpdate => "追加或更新",
            Self::SkipDuplicates => "跳过重复记录",
            Self::Delete => "删除匹配记录",
            Self::Copy => "复制（清空目标后导入）",
        }
    }
    pub fn needs_key(self) -> bool {
        matches!(
            self,
            Self::Update | Self::AppendUpdate | Self::SkipDuplicates | Self::Delete
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub mode: Mode,
    pub transaction: bool,
    pub continue_on_error: bool,
    pub batch_size: usize,
    pub binary_base64: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            mode: Mode::Append,
            transaction: true,
            continue_on_error: false,
            batch_size: 1000,
            binary_base64: false,
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub read: u64,
    pub inserted: u64,
    pub updated: u64,
    pub deleted: u64,
    pub skipped: u64,
    pub failed: u64,
    pub committed: u64,
    pub cancelled: bool,
    pub uncertain: bool,
    pub errors: Vec<String>,
    pub error: Option<String>,
    pub changed: Vec<TableId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub version: u32,
    pub source: Source,
    pub targets: Vec<Target>,
    pub options: Options,
}
impl Profile {
    pub fn redacted(source: &Source, targets: &[Target], options: &Options) -> Self {
        Self {
            version: 1,
            source: source.without_secrets(),
            targets: targets
                .iter()
                .cloned()
                .map(|mut t| {
                    t.expected = None;
                    t
                })
                .collect(),
            options: options.clone(),
        }
    }
}
