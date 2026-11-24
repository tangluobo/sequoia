//! Self-hosted collaboration: HTTP adapter, durable local outbox and cancellable actor.
mod accounts;
mod api;
pub use accounts::Account;
mod repository;
mod worker;
pub use api::{Api, HttpError};
pub use repository::{Cached, Repository};
pub use tomato_core::collaboration::*;
pub fn connection_stamp(name: &str, body: &serde_json::Value) -> String {
    let mut value = serde_json::json!({"name":name,"body":body});
    value.sort_all_objects();
    crate::queries::stamp(&serde_json::to_vec(&value).unwrap_or_default())
}
pub use worker::{Client, Command, Detail, ProjectAction, View};

#[derive(serde::Serialize, serde::Deserialize)]
pub struct Login {
    pub token: String,
    pub user: User,
    pub expires: i64,
}
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Event {
    pub seq: i64,
    pub project_id: String,
    pub user_id: String,
    pub object_id: String,
    pub action: String,
    pub created: i64,
}
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Events {
    pub cursor: i64,
    pub events: Vec<Event>,
}
