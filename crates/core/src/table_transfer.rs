//! Transfer plan is reviewable SQL plus schema stamps; row data never enters the UI.
use crate::{Connection, workbench::TableId};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub source: Connection,
    pub target: Connection,
    pub source_schema: String,
    pub target_schema: String,
    pub tables: Vec<String>,
    pub structure: bool,
    pub data: bool,
    pub drop_existing: bool,
    pub add: bool,
    pub drop: bool,
    pub modify: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TablePlan {
    pub source: TableId,
    pub target: TableId,
    pub sql: Vec<String>,
    pub data: bool,
    /// When present, the database copies the rows within a single session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_sql: Option<String>,
    pub stamp: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub tables: Vec<TablePlan>,
}
impl Plan {
    pub fn preview(&self) -> String {
        self.tables
            .iter()
            .map(|table| {
                format!(
                    "-- {}\n{}{}",
                    table.source.name,
                    table
                        .sql
                        .iter()
                        .map(|s| format!("{s};\n"))
                        .collect::<String>(),
                    if let Some(statement) = &table.data_sql {
                        format!("{statement};\n")
                    } else if table.data {
                        "-- 复制表数据 (INSERT)\n".into()
                    } else {
                        String::new()
                    }
                )
            })
            .collect()
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub succeeded: Vec<TableId>,
    pub failed: Vec<(TableId, String)>,
    pub records: u64,
    pub cancelled: bool,
}
