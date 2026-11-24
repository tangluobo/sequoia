//! Database backup requests are distinct from filesystem commands and archive I/O.
use super::*;
use crate::workbench::TableId;
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Table,
    View,
    Function,
    Event,
}
impl Kind {
    pub fn archive_name(self) -> &'static str {
        match self {
            Self::Table => "Table",
            Self::View => "View",
            Self::Function => "Function",
            Self::Event => "Event",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Table => "表",
            Self::View => "视图",
            Self::Function => "函数",
            Self::Event => "事件",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub table: TableId,
    pub kind: Kind,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Create {
    pub scope: Scope,
    pub filename: String,
    pub objects: Vec<Selection>,
    pub comment: String,
    #[serde(default)]
    pub lock_tables: bool,
    #[serde(default)]
    pub single_transaction: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Restore {
    pub scope: Scope,
    pub name: String,
    #[serde(default)]
    pub namespace: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Operation {
    Create(Create),
    Restore(Restore),
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub path: PathBuf,
    pub objects: u64,
    pub records: u64,
    pub elapsed_ms: u64,
}
impl Operation {
    pub fn retained_bytes(&self) -> usize {
        let scope = match self {
            Self::Create(request) => &request.scope,
            Self::Restore(request) => &request.scope,
        };
        let base = scope
            .connection
            .capacity()
            .saturating_add(scope.database.capacity())
            .saturating_add(scope.path.capacity());
        base.saturating_add(match self {
            Self::Create(request) => request.objects.iter().fold(
                request
                    .filename
                    .capacity()
                    .saturating_add(request.comment.capacity())
                    .saturating_add(
                        request
                            .objects
                            .capacity()
                            .saturating_mul(std::mem::size_of::<Selection>()),
                    ),
                |sum, object| {
                    sum.saturating_add(object.table.name.capacity())
                        .saturating_add(object.table.schema.capacity())
                },
            ),
            Self::Restore(request) => request
                .name
                .capacity()
                .saturating_add(request.namespace.as_ref().map_or(0, String::capacity)),
        })
    }
}
