//! Filesystem requests retain the original backup directory and basename rules.
use super::*;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub directory: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileRequest {
    pub scope: Scope,
    pub action: FileAction,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FileAction {
    List,
    Info { name: String },
    CreateDirectory { name: String },
    Rename { item: Item, name: String },
    Delete { items: Vec<Item> },
    Directory,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum FileOutput {
    Listed {
        scope: Scope,
        items: Vec<Item>,
    },
    Info {
        scope: Scope,
        name: String,
        metadata: Box<Metadata>,
        bytes: u64,
    },
    Changed {
        scope: Scope,
        renamed: Option<(Item, Item)>,
        deleted: Vec<Item>,
        errors: Vec<String>,
    },
    Directory {
        path: PathBuf,
    },
}
impl Scope {
    pub fn child(&self, name: &str) -> Result<Self> {
        let name = super::sanitize(name);
        let path = if self.path.is_empty() {
            name
        } else {
            format!("{}/{name}", self.path)
        };
        let scope = Self {
            path,
            ..self.clone()
        };
        scope.directory()?;
        Ok(scope)
    }
    pub fn renamed_file(&self, name: &str) -> Result<PathBuf> {
        self.file(&super::sanitize(name))
    }
}
impl FileRequest {
    pub fn retained_bytes(&self) -> usize {
        let bytes = self
            .scope
            .connection
            .capacity()
            .saturating_add(self.scope.database.capacity())
            .saturating_add(self.scope.path.capacity());
        bytes.saturating_add(match &self.action {
            FileAction::List | FileAction::Directory => 0,
            FileAction::Info { name } | FileAction::CreateDirectory { name } => name.capacity(),
            FileAction::Rename { item, name } => {
                item.name.capacity().saturating_add(name.capacity())
            }
            FileAction::Delete { items } => items.iter().fold(
                items.capacity().saturating_mul(std::mem::size_of::<Item>()),
                |sum, item| sum.saturating_add(item.name.capacity()),
            ),
        })
    }
}
