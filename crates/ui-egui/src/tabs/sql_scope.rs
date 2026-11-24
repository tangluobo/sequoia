//! A result retains the execution connection while the toolbar changes selection.
use std::sync::Arc;
#[derive(Clone)]
pub struct ResultContext {
    pub connection: Arc<tomato_core::Connection>,
    pub namespace: String,
}
impl ResultContext {
    pub fn new(connection: &tomato_core::Connection, namespace: &str) -> Self {
        let namespace = if connection.kind() == "MYSQL" && !connection.text("database").is_empty() {
            connection.text("database")
        } else if connection.kind() == "HIVE" {
            tomato_core::database_scope::hive_name(connection.text("database"))
        } else {
            namespace
        };
        Self {
            connection: Arc::new(connection.clone()),
            namespace: namespace.into(),
        }
    }
}

#[derive(Default)]
pub struct Selectors {
    pub(crate) connection: String,
    pub(crate) database: String,
    pub(crate) identity: Option<(String, String)>,
    pub(crate) editors: [super::text_state::Editor; 2],
    pub selected: bool,
    pub databases: Arc<Vec<String>>,
    pub catalog_generation: u64,
}
impl Selectors {
    pub fn release(&self, ctx: &egui::Context) {
        for editor in &self.editors {
            editor.release(ctx);
        }
    }
}
