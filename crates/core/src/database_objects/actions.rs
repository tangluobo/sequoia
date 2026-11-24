use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub table: TableId,
    pub view: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mutation {
    Drop,
    Clear,
    Truncate,
    Rename { name: String },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub targets: Vec<Target>,
    pub mutation: Mutation,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChangeReport {
    pub succeeded: Vec<TableId>,
    pub failures: Vec<(TableId, String)>,
    pub affected: u64,
}
