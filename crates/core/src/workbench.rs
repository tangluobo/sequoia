//! Typed database workbench requests, independent of SQL drivers and UI widgets.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableId {
    pub schema: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableInfo {
    pub table: TableId,
    pub view: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Cell {
    Null,
    Text(String),
    Integer(i64),
    Real(f64),
    Boolean(bool),
    Binary(Vec<u8>),
}
impl Cell {
    pub fn display(&self) -> String {
        match self {
            Self::Null => "NULL".into(),
            Self::Text(text) => text.clone(),
            Self::Integer(value) => value.to_string(),
            Self::Real(value) => value.to_string(),
            Self::Boolean(value) => value.to_string(),
            Self::Binary(bytes) => bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub primary_key: u32,
    pub generated: bool,
    pub auto_increment: bool,
    pub binary: bool,
    /// PostgreSQL's actual type identity, safely quoted when casting parameters.
    pub type_schema: String,
    pub type_name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableDefinition {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub comments: std::collections::BTreeMap<String, String>,
    pub table: TableId,
    pub columns: Vec<Column>,
    pub view: bool,
    pub transactional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub column: String,
    pub descending: bool,
}

/// Server-side table criteria. Names are validated against live metadata before SQL generation.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TableQuery {
    pub filters: Vec<ColumnFilter>,
    pub sorts: Vec<Sort>,
    pub search: String,
    pub match_any: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColumnFilter {
    pub column: String,
    pub condition: crate::result_filter::Condition,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PageRequest {
    pub table: TableId,
    pub offset: u64,
    pub limit: u32,
    pub sort: Option<Sort>,
    #[serde(default)]
    pub query: TableQuery,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TablePage {
    pub definition: TableDefinition,
    pub rows: Vec<Vec<Cell>>,
    pub total: u64,
    pub offset: u64,
    pub limit: u32,
    pub truncated: bool,
    #[serde(default)]
    pub query: TableQuery,
    /// Captured after execution; never regenerated from unapplied UI criteria.
    #[serde(default)]
    pub sql: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowChange {
    /// None omits the column and uses its database default; Some(Null) is SQL NULL.
    Insert {
        values: Vec<Option<Cell>>,
    },
    Update {
        original: Vec<Cell>,
        values: Vec<Cell>,
    },
    Delete {
        original: Vec<Cell>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionAction {
    Begin,
    Commit,
    Rollback,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum DatabaseAction {
    Design {
        table: TableId,
    },
    DesignStructure {
        table: TableId,
    },
    DesignSql {
        table: TableId,
    },
    SaveDesign {
        edit: Box<crate::table_design::alter::Edit>,
    },
    SaveStructure {
        edit: Box<crate::table_design::alter::Edit>,
    },
    DesignInfo {
        table: TableId,
        page: crate::table_design::alter::InfoPage,
    },
    DesignMetadata,
    CreateTable {
        request: crate::table_design::Create,
    },
    Objects {
        schema: String,
    },
    ObjectHeaders {
        schema: String,
    },
    ObjectFields {
        schema: String,
        #[serde(with = "crate::database_objects::shared_catalog")]
        catalog: std::sync::Arc<crate::database_objects::Catalog>,
    },
    ObjectMutation {
        schema: String,
        request: crate::database_objects::Request,
    },
    Completion {
        schema: String,
    },
    Databases,
    Schemas,
    Tables {
        schema: String,
    },
    SelectSchema {
        schema: String,
    },
    Page {
        request: PageRequest,
    },
    DeleteRows {
        table: TableId,
        rows: Vec<(usize, Vec<Cell>)>,
    },
    Apply {
        table: TableId,
        changes: Vec<RowChange>,
        request: PageRequest,
    },
    Definition {
        table: TableId,
    },
    Transaction {
        action: TransactionAction,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum DatabaseOutput {
    Design(Box<crate::table_design::alter::Snapshot>),
    DesignSql {
        table: TableId,
        sql: String,
    },
    DesignInfo {
        page: crate::table_design::alter::InfoPage,
        info: crate::table_design::alter::InfoTable,
    },
    DesignMetadata(crate::table_design::Metadata),
    TableCreated(TableId),
    Objects(crate::database_objects::Catalog),
    ObjectHeaders(crate::database_objects::Catalog),
    ObjectFields {
        schema: String,
        fields: Vec<crate::database_objects::Fields>,
    },
    ObjectsChanged(crate::database_objects::ChangeReport),
    Completion(crate::sql_completion::Metadata),
    Databases(Vec<String>),
    Schemas(Vec<String>),
    Tables(Vec<TableInfo>),
    Page(TablePage),
    Deleted {
        table: TableId,
        rows: Vec<(usize, Vec<Cell>)>,
        affected: u64,
    },
    Applied {
        affected: u64,
        page: Option<TablePage>,
        refresh_error: Option<String>,
    },
    Definition(TableDefinition),
    Transaction(bool),
}
