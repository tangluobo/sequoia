//! Native MongoDB adapter for the original collection workbench.
use anyhow::{Context, Result, ensure};
use bson::{Bson, Document, doc};
use futures_util::TryStreamExt;
use mongodb::{
    Client, IndexModel,
    options::{ClientOptions, Credential, IndexOptions},
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};
use tomato_core::Connection;
const BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Target {
    pub database: String,
    pub collection: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Action {
    Databases,
    Collections {
        database: String,
    },
    Find {
        target: Target,
        filter: String,
        skip: u64,
        limit: u32,
    },
    Insert {
        target: Target,
        json: String,
    },
    Replace {
        target: Target,
        original: String,
        json: String,
    },
    Delete {
        target: Target,
        json: String,
    },
    Command {
        database: String,
        json: String,
    },
    CreateCollection {
        target: Target,
    },
    RenameCollection {
        target: Target,
        name: String,
    },
    DropCollection {
        target: Target,
    },
    Indexes {
        target: Target,
    },
    CreateIndex {
        target: Target,
        keys: String,
        name: String,
        unique: bool,
    },
    DropIndex {
        target: Target,
        name: String,
    },
    Validator {
        target: Target,
    },
    UpdateValidator {
        target: Target,
        json: String,
    },
}
#[derive(Debug, Serialize)]
pub enum Output {
    Databases(Vec<String>),
    Collections {
        database: String,
        names: Vec<String>,
    },
    Documents {
        target: Target,
        documents: Vec<String>,
        truncated: bool,
    },
    Document {
        target: Target,
        json: String,
    },
    Indexes {
        target: Target,
        indexes: Vec<String>,
    },
    Validator {
        target: Target,
        json: String,
    },
    Text(String),
    Changed {
        target: Target,
        kind: String,
    },
}
struct Session {
    client: Client,
    _tunnel: Option<crate::terminal::ssh::tunnel::Tunnel>,
}
#[derive(Default)]
pub struct Sessions {
    sessions: HashMap<String, Session>,
}
pub fn parse(text: &str) -> Result<Document> {
    ensure!(text.len() <= BYTES, "MongoDB JSON 超过 8 MiB");
    let text = if text.trim().is_empty() { "{}" } else { text };
    let json: serde_json::Value = serde_json::from_str(text).context("必须是有效的 JSON 对象")?;
    ensure!(json.is_object(), "必须是 JSON 对象");
    match Bson::try_from(json)? {
        Bson::Document(document) => Ok(document),
        _ => anyhow::bail!("必须是 JSON 对象"),
    }
}
pub fn pretty(document: Document) -> Result<String> {
    let text = serde_json::to_string_pretty(&Bson::Document(document).into_relaxed_extjson())?;
    ensure!(text.len() <= BYTES, "MongoDB 文档超过 8 MiB 编辑上限");
    Ok(text)
}
pub fn replacement(original: &str, json: &str) -> Result<(Bson, Document)> {
    let original = parse(original)?;
    let id = original
        .get("_id")
        .filter(|id| !matches!(id, Bson::Null))
        .context("文档缺少 _id，无法安全定位并保存")?
        .clone();
    let mut next = parse(json)?;
    if let Some(next_id) = next.get("_id").filter(|id| !matches!(id, Bson::Null)) {
        ensure!(next_id == &id, "MongoDB 不允许修改文档的 _id");
    } else {
        next.insert("_id", id.clone());
    }
    Ok((id, next))
}
fn name(text: &str) -> Result<&str> {
    let text = text.trim();
    ensure!(
        !text.is_empty() && !text.contains('\0'),
        "数据库/集合/索引名不能为空或包含 NUL"
    );
    Ok(text)
}
fn auth_database(connection: &Connection) -> &str {
    let database = connection.text("database").trim();
    if database.is_empty() {
        "admin"
    } else {
        database
    }
}
async fn settings(connection: &Connection, routed: Option<&Connection>) -> Result<ClientOptions> {
    let host = connection.text("host").trim();
    let uri = if host.starts_with("mongodb://") || host.starts_with("mongodb+srv://") {
        ensure!(
            !connection.flag("useSshTunnel"),
            "连接 URI 模式暂不支持 SSH 通道，请填写单个 MongoDB 主机地址"
        );
        host.to_owned()
    } else {
        let endpoint = routed.unwrap_or(connection);
        let mut uri = url::Url::parse("mongodb://localhost")?;
        uri.set_host(Some(endpoint.text("host").trim()))?;
        uri.set_port(Some(u16::try_from(endpoint.number("port"))?))
            .map_err(|_| anyhow::anyhow!("MongoDB 端口无效"))?;
        uri.to_string()
    };
    let mut options = ClientOptions::parse(uri).await?;
    options.connect_timeout = Some(Duration::from_secs(5));
    options.server_selection_timeout = Some(Duration::from_secs(5));
    options.max_pool_size = Some(4);
    options.min_pool_size = Some(0);
    options.max_idle_time = Some(Duration::from_secs(60));
    if routed.is_some() {
        options.direct_connection = Some(true);
    }
    if !connection.text("username").trim().is_empty() {
        options.credential = Some(
            Credential::builder()
                .username(Some(connection.text("username").trim().to_owned()))
                .password(Some(connection.text("password").to_owned()))
                .source(Some(auth_database(connection).to_owned()))
                .build(),
        );
    }
    Ok(options)
}
impl Sessions {
    async fn client(&mut self, connection: &Connection) -> Result<Client> {
        if !self.sessions.contains_key(&connection.id) {
            crate::init_crypto();
            let uri = connection.text("host").starts_with("mongodb://")
                || connection.text("host").starts_with("mongodb+srv://");
            ensure!(
                !uri || !connection.flag("useSshTunnel"),
                "连接 URI 模式暂不支持 SSH 通道"
            );
            let tunnel = if connection.flag("useSshTunnel") {
                Some(crate::terminal::ssh::tunnel::Tunnel::open(connection).await?)
            } else {
                None
            };
            let routed = tunnel.as_ref().map(|t| t.routed(connection));
            let client = Client::with_options(settings(connection, routed.as_ref()).await?)?;
            if let Err(error) = client
                .database(auth_database(connection))
                .run_command(doc! {"ping":1})
                .await
            {
                client.shutdown().await;
                return Err(error.into());
            }
            self.sessions.insert(
                connection.id.clone(),
                Session {
                    client,
                    _tunnel: tunnel,
                },
            );
        }
        Ok(self
            .sessions
            .get(&connection.id)
            .context("MongoDB 会话不存在")?
            .client
            .clone())
    }
    pub async fn close(&mut self, id: &str) {
        if let Some(session) = self.sessions.remove(id) {
            session.client.shutdown().await;
        }
    }
    pub async fn execute(&mut self, connection: &Connection, action: Action) -> Result<Output> {
        tokio::time::timeout(Duration::from_secs(30), self.run(connection, action))
            .await
            .context("MongoDB 操作超时")?
    }
    async fn run(&mut self, connection: &Connection, action: Action) -> Result<Output> {
        let client = self.client(connection).await?;
        match action {
            Action::Databases => {
                let mut names = client.list_database_names().await?;
                ensure!(
                    names.len() <= 10_000 && names.iter().map(String::len).sum::<usize>() <= BYTES,
                    "MongoDB 数据库目录过大"
                );
                names.sort_by_key(|s| s.to_lowercase());
                Ok(Output::Databases(names))
            }
            Action::Collections { database } => {
                let mut cursor = client
                    .database(name(&database)?)
                    .list_collections()
                    .batch_size(100)
                    .await?;
                let mut names = Vec::new();
                let mut bytes = 0;
                while let Some(collection) = cursor.try_next().await? {
                    bytes += collection.name.len();
                    ensure!(
                        names.len() < 10_000 && bytes <= BYTES,
                        "MongoDB 集合目录过大"
                    );
                    names.push(collection.name);
                }
                names.sort_by_key(|s| s.to_lowercase());
                Ok(Output::Collections { database, names })
            }
            Action::Command { database, json } => Ok(Output::Text(pretty(
                client
                    .database(name(&database)?)
                    .run_command(parse(&json)?)
                    .await?,
            )?)),
            other => {
                let target = other.target().context("MongoDB 操作缺少集合")?.clone();
                let database = client.database(name(&target.database)?);
                let collection = database.collection::<Document>(name(&target.collection)?);
                match other {
                    Action::Find {
                        filter,
                        skip,
                        limit,
                        ..
                    } => {
                        let mut cursor = collection
                            .find(parse(&filter)?)
                            .skip(skip)
                            .limit(i64::from(limit.clamp(1, 1000)))
                            .batch_size(50)
                            .max_time(Duration::from_secs(30))
                            .await?;
                        let mut documents = Vec::new();
                        let mut bytes = 0usize;
                        let mut truncated = false;
                        while let Some(document) = cursor.try_next().await? {
                            let text = pretty(document)?;
                            if bytes.saturating_add(text.len()) > BYTES {
                                truncated = true;
                                break;
                            }
                            bytes += text.len();
                            documents.push(text);
                        }
                        Ok(Output::Documents {
                            target,
                            documents,
                            truncated,
                        })
                    }
                    Action::Insert { json, .. } => {
                        let mut document = parse(&json)?;
                        document
                            .entry("_id")
                            .or_insert_with(|| Bson::ObjectId(bson::oid::ObjectId::new()));
                        let json = pretty(document.clone())?;
                        collection.insert_one(&document).await?;
                        Ok(Output::Document { target, json })
                    }
                    Action::Replace { original, json, .. } => {
                        let (id, document) = replacement(&original, &json)?;
                        let json = pretty(document.clone())?;
                        let result = collection.replace_one(doc! {"_id":id}, &document).await?;
                        ensure!(result.matched_count > 0, "原文档已不存在，请刷新后重试");
                        Ok(Output::Document { target, json })
                    }
                    Action::Delete { json, .. } => {
                        let document = parse(&json)?;
                        let id = document
                            .get("_id")
                            .filter(|id| !matches!(id, Bson::Null))
                            .context("文档缺少 _id，无法安全删除")?
                            .clone();
                        let result = collection.delete_one(doc! {"_id":id}).await?;
                        Ok(Output::Changed {
                            target,
                            kind: if result.deleted_count > 0 {
                                "documents"
                            } else {
                                "documents_missing"
                            }
                            .into(),
                        })
                    }
                    Action::CreateCollection { .. } => {
                        database
                            .create_collection(name(&target.collection)?)
                            .await?;
                        Ok(Output::Changed {
                            target,
                            kind: "collections".into(),
                        })
                    }
                    Action::RenameCollection { name: new, .. } => {
                        client.database("admin").run_command(doc!{"renameCollection":format!("{}.{}",target.database,target.collection),"to":format!("{}.{}",target.database,name(&new)?)}).await?;
                        Ok(Output::Changed {
                            target: Target {
                                collection: new.trim().into(),
                                ..target
                            },
                            kind: "renamed".into(),
                        })
                    }
                    Action::DropCollection { .. } => {
                        collection.drop().await?;
                        Ok(Output::Changed {
                            target,
                            kind: "dropped".into(),
                        })
                    }
                    Action::Indexes { .. } => {
                        let mut cursor = database
                            .run_cursor_command(
                                doc! {"listIndexes":&target.collection,"cursor":{"batchSize":50}},
                            )
                            .await?;
                        let mut indexes = Vec::new();
                        let mut bytes = 0usize;
                        while let Some(index) = cursor.try_next().await? {
                            let key = index.get_str("name").unwrap_or_default().to_lowercase();
                            let text = pretty(index)?;
                            bytes += text.len();
                            ensure!(
                                indexes.len() < 1000 && bytes <= BYTES,
                                "MongoDB 索引列表过大"
                            );
                            indexes.push((key, text));
                        }
                        indexes.sort_by(|a, b| a.0.cmp(&b.0));
                        Ok(Output::Indexes {
                            target,
                            indexes: indexes.into_iter().map(|(_, text)| text).collect(),
                        })
                    }
                    Action::CreateIndex {
                        keys,
                        name: index_name,
                        unique,
                        ..
                    } => {
                        let options = IndexOptions::builder()
                            .name((!index_name.trim().is_empty()).then(|| index_name.trim().into()))
                            .unique(unique)
                            .build();
                        let index = IndexModel::builder()
                            .keys(parse(&keys)?)
                            .options(options)
                            .build();
                        collection.create_index(index).await?;
                        Ok(Output::Changed {
                            target,
                            kind: "indexes".into(),
                        })
                    }
                    Action::DropIndex {
                        name: index_name, ..
                    } => {
                        ensure!(index_name != "_id_", "不能删除 MongoDB 的 _id_ 索引");
                        collection.drop_index(name(&index_name)?).await?;
                        Ok(Output::Changed {
                            target,
                            kind: "indexes".into(),
                        })
                    }
                    Action::Validator { .. } => {
                        let mut cursor = database
                            .list_collections()
                            .filter(doc! {"name":target.collection.clone()})
                            .await?;
                        let validator = if let Some(metadata) = cursor.try_next().await? {
                            metadata.options.validator.unwrap_or_default()
                        } else {
                            Document::new()
                        };
                        Ok(Output::Validator {
                            target,
                            json: pretty(validator)?,
                        })
                    }
                    Action::UpdateValidator { json, .. } => {
                        database.run_command(doc!{"collMod":target.collection.clone(),"validator":parse(&json)?,"validationLevel":"strict","validationAction":"error"}).await?;
                        Ok(Output::Changed {
                            target,
                            kind: "validator".into(),
                        })
                    }
                    _ => unreachable!(),
                }
            }
        }
    }
}
impl Action {
    pub fn target(&self) -> Option<&Target> {
        match self {
            Self::Find { target, .. }
            | Self::Insert { target, .. }
            | Self::Replace { target, .. }
            | Self::Delete { target, .. }
            | Self::CreateCollection { target }
            | Self::RenameCollection { target, .. }
            | Self::DropCollection { target }
            | Self::Indexes { target }
            | Self::CreateIndex { target, .. }
            | Self::DropIndex { target, .. }
            | Self::Validator { target }
            | Self::UpdateValidator { target, .. } => Some(target),
            _ => None,
        }
    }
    pub fn queued_bytes(&self) -> usize {
        let target = self
            .target()
            .map_or(0, |t| t.database.capacity() + t.collection.capacity());
        target
            + match self {
                Self::Databases => 0,
                Self::Collections { database } => database.capacity(),
                Self::Command { database, json } => database.capacity() + json.capacity(),
                Self::Find { filter, .. } => filter.capacity(),
                Self::Insert { json, .. }
                | Self::Delete { json, .. }
                | Self::UpdateValidator { json, .. } => json.capacity(),
                Self::Replace { original, json, .. } => original.capacity() + json.capacity(),
                Self::RenameCollection { name, .. } | Self::DropIndex { name, .. } => {
                    name.capacity()
                }
                Self::CreateIndex { keys, name, .. } => keys.capacity() + name.capacity(),
                _ => 0,
            }
    }
}
