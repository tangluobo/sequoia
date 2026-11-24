//! Command bus / facade shared by GUI and CLI. Adapters own protocol details.
use crate::{Databases, QueryResult, files, network, tools};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf, pin::Pin};
use tomato_core::Connection;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", content = "params", rename_all = "snake_case")]
pub enum Command {
    AdbPair(crate::remote_desktop::AdbPairing),
    GameDungeons {
        path: PathBuf,
        action: crate::game_dungeons::Action,
        revision: Option<String>,
    },
    ConnectionImport {
        request: crate::connections::Request,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    ConnectionExport {
        source: Option<PathBuf>,
        path: PathBuf,
        prepared: tomato_core::config::export::Prepared,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    InputShare {
        action: crate::input_share::Action,
    },
    FileDocument {
        #[serde(skip)]
        client: Option<files::document::Client>,
        action: files::document::Action,
    },
    Development {
        path: PathBuf,
        action: crate::development::Action,
    },
    Cloud {
        connection: Connection,
        action: crate::cloud::Action,
        #[serde(default)]
        ddns_path: Option<PathBuf>,
    },
    CloudBuckets {
        connection: Connection,
    },
    Certificate {
        connection: Connection,
        request: Box<crate::cloud::certificate::Request>,
        #[serde(default)]
        renewal: Option<(PathBuf, crate::cloud::certificate::renewal::Options)>,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    MessageQueue {
        connection: Connection,
        action: crate::mq::Action,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    DatabaseAdmin {
        connection: Connection,
        request: tomato_core::database_admin::Request,
    },
    TransferTables {
        request: Box<tomato_core::table_transfer::Request>,
        accepted: Option<tomato_core::table_transfer::Plan>,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    PasteTables {
        request: Box<tomato_core::table_transfer::Request>,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    BackupObjects {
        connection: Connection,
        namespace: String,
        #[serde(default)]
        schema: Option<String>,
    },
    AiSql {
        request: Box<crate::ai_sql::Request>,
    },
    BackupDatabase {
        connection: Connection,
        root: PathBuf,
        operation: tomato_core::backup::Operation,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    BackupFiles {
        root: PathBuf,
        request: tomato_core::backup::FileRequest,
    },
    NamedQueries {
        root: PathBuf,
        request: tomato_core::queries::Request,
    },
    ExtractResources {
        options: crate::extractor::Options,
    },
    Music {
        request: tools::music::Request,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    Trae {
        path: PathBuf,
        action: crate::trae::Action,
    },
    Mongo {
        connection: Connection,
        action: crate::mongo::Action,
    },
    RedisData {
        connection: Connection,
        action: crate::redis_data::Action,
    },
    Hosts {
        config_path: PathBuf,
        system_path: PathBuf,
        action: crate::hosts::Action,
    },
    Database {
        connection: Connection,
        action: tomato_core::workbench::DatabaseAction,
    },
    Sql {
        connection: Connection,
        sql: String,
    },
    SqlScript {
        connection: Connection,
        sql: String,
        mode: crate::database::script::Mode,
    },
    SaveQueryRows {
        connection: Connection,
        result: usize,
        editing: crate::database::query_rows::Editing,
        changes: Vec<crate::database::query_rows::Change>,
    },
    Redis {
        connection: Connection,
        args: Vec<String>,
    },
    ListFiles {
        path: PathBuf,
    },
    ListVisibleFiles {
        path: PathBuf,
    },
    LocalTree {
        root: PathBuf,
        action: files::tree::Action,
    },
    ObjectTree {
        connection: Connection,
        action: files::object_tree::Request,
    },
    Markdown {
        source: String,
    },
    MarkdownHtml {
        source: String,
        title: String,
        path: PathBuf,
    },
    MarkdownImage {
        document: PathBuf,
        source: String,
    },
    CreateDirectory {
        parent: PathBuf,
        name: String,
    },
    ExportCsv {
        path: PathBuf,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    ImportTableRows {
        path: PathBuf,
        definition: tomato_core::workbench::TableDefinition,
        remaining: usize,
    },
    DesktopShortcut {
        destination: PathBuf,
        name: String,
        executable: PathBuf,
        icon: PathBuf,
    },
    SaveDesktopShortcut {
        destination: PathBuf,
        shortcut: Box<tools::shortcut::Shortcut>,
    },
    ReadText {
        path: PathBuf,
    },
    WriteText {
        path: PathBuf,
        text: String,
    },
    Json {
        text: String,
        action: String,
    },
    JsonHistory {
        path: PathBuf,
        action: crate::json_history::Action,
    },
    ConvertTool {
        request: tools::conversion::Request,
        #[serde(skip)]
        control: Option<crate::backup::Control>,
    },
    ConvertImage {
        source: PathBuf,
        destination: PathBuf,
        width: u32,
    },
    MakeIcon {
        source: PathBuf,
        destination: PathBuf,
        tolerance: u8,
        size: u32,
    },
    ConvertDataset {
        source: PathBuf,
        destination: PathBuf,
    },
    Download {
        url: String,
        destination: PathBuf,
    },
    Disconnect {
        id: String,
    },
    DisconnectDatabase {
        id: String,
        database: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Output {
    GameDungeons(crate::game_dungeons::Snapshot),
    ConnectionImport(crate::connections::Output),
    InputShare(crate::input_share::Snapshot),
    Development(crate::development::Output),
    Cloud(crate::cloud::Output),
    CloudBuckets(Vec<String>),
    Certificate(String),
    MessageQueue(crate::mq::Output),
    DatabaseAdmin(tomato_core::database_admin::Output),
    TransferPlan(tomato_core::table_transfer::Plan),
    TransferReport(tomato_core::table_transfer::Report),
    BackupObjects(Vec<tomato_core::backup::Selection>),
    AiSql(String),
    BackupDatabase(tomato_core::backup::Report),
    BackupFiles(tomato_core::backup::FileOutput),
    NamedQueries(tomato_core::queries::Output),
    Resources(crate::extractor::Report),
    Music(tools::music::Output),
    Conversion(tools::conversion::Report),
    Trae(crate::trae::Output),
    Mongo(crate::mongo::Output),
    RedisData(crate::redis_data::Output),
    Hosts(crate::hosts::Snapshot),
    JsonHistory(crate::json_history::Output),
    Database(tomato_core::workbench::DatabaseOutput),
    Text(String),
    Queries(Vec<QueryResult>),
    Script(Box<crate::database::script::Script>),
    SavedQueryRows {
        result: usize,
        affected: u64,
        deleting: bool,
    },
    Files(Vec<files::Entry>),
    LocalTree(files::tree::Changed),
    Markdown(crate::markdown::Document),
    MarkdownImage(crate::markdown::Picture),
    StagedRows(Vec<Vec<Option<tomato_core::workbench::Cell>>>),
}

pub type CommandFuture<'a> = Pin<Box<dyn Future<Output = Result<Output>> + Send + 'a>>;

/// Adapter port allows alternative engines without changing callers.
pub trait CommandHandler: Send {
    fn execute(&mut self, command: Command) -> CommandFuture<'_>;
}

#[derive(Default)]
pub struct Engine {
    input_share: crate::input_share::Manager,
    development: crate::development::Manager,
    databases: Databases,
    mongo: crate::mongo::Sessions,
}

impl CommandHandler for Engine {
    fn execute(&mut self, command: Command) -> CommandFuture<'_> {
        crate::init_crypto();
        match command {
            Command::AdbPair(request) => Box::pin(async move {
                request.pair().await?;
                Ok(Output::Text("无线配对成功".into()))
            }),
            Command::GameDungeons {
                path,
                action,
                revision,
            } => Box::pin(async move {
                Ok(Output::GameDungeons(
                    tokio::task::spawn_blocking(move || {
                        crate::game_dungeons::execute(path, action, revision)
                    })
                    .await??,
                ))
            }),
            Command::Music { request, control } => Box::pin(async move {
                Ok(Output::Music(
                    tools::music::execute(request, control.unwrap_or_default()).await?,
                ))
            }),
            Command::ConvertTool { request, control } => Box::pin(async move {
                Ok(Output::Conversion(
                    tools::conversion::execute(request, control.unwrap_or_default()).await?,
                ))
            }),
            Command::InputShare { action } => {
                Box::pin(
                    async move { Ok(Output::InputShare(self.input_share.execute(action).await?)) },
                )
            }
            Command::Development { path, action } => Box::pin(async move {
                Ok(Output::Development(
                    self.development.execute(path, action).await?,
                ))
            }),
            Command::Cloud {
                connection,
                action,
                ddns_path,
            } => Box::pin(async move {
                Ok(Output::Cloud(
                    crate::cloud::execute_at(
                        &connection,
                        action,
                        ddns_path.unwrap_or_else(crate::cloud::ddns::path),
                    )
                    .await?,
                ))
            }),
            Command::MessageQueue {
                connection,
                action,
                control,
            } => Box::pin(async move {
                let control = control.unwrap_or_default();
                tokio::select! {biased;_=control.wait_cancelled()=>anyhow::bail!("消息操作已取消"),result=crate::mq::execute(&connection,action)=>Ok(Output::MessageQueue(result?))}
            }),
            Command::Certificate {
                connection,
                request,
                renewal,
                control,
            } => Box::pin(async move {
                let control = control.unwrap_or_default();
                let message = if let Some((profile, options)) = renewal {
                    crate::cloud::certificate::renewal::issue(
                        profile, connection, *request, options, &control,
                    )
                    .await?
                } else {
                    crate::cloud::certificate::issue(&connection, *request, &control).await?
                };
                Ok(Output::Certificate(message))
            }),
            Command::CloudBuckets { mut connection } => Box::pin(async move {
                connection.set("bucket", "");
                let output = crate::files::storage::Storage::connect(&connection)
                    .await?
                    .list("/")
                    .await?;
                if let crate::files::operations::Output::Directory { entries, .. } = output {
                    Ok(Output::CloudBuckets(
                        entries.into_iter().map(|e| e.name).collect(),
                    ))
                } else {
                    anyhow::bail!("存储未返回 Bucket 目录")
                }
            }),
            Command::DatabaseAdmin {
                connection,
                request,
            } => Box::pin(async move {
                Ok(Output::DatabaseAdmin(
                    self.databases.admin(&connection, request).await?,
                ))
            }),
            Command::TransferTables {
                request,
                accepted,
                control,
            } => Box::pin(async move {
                let control = control.unwrap_or_default();
                if let Some(plan) = accepted {
                    Ok(Output::TransferReport(
                        crate::database::transfer::execute(&request, &plan, &control).await?,
                    ))
                } else {
                    Ok(Output::TransferPlan(
                        crate::database::transfer::preview(&request, &control).await?,
                    ))
                }
            }),
            Command::PasteTables { request, control } => Box::pin(async move {
                Ok(Output::TransferReport(
                    crate::database::transfer::paste(&request, &control.unwrap_or_default())
                        .await?,
                ))
            }),
            Command::AiSql { request } => {
                Box::pin(async move { Ok(Output::AiSql(crate::ai_sql::generate(*request).await?)) })
            }
            Command::Trae { path, action } => {
                Box::pin(async move { Ok(Output::Trae(crate::trae::execute(path, action).await?)) })
            }
            Command::Mongo { connection, action } => Box::pin(async move {
                Ok(Output::Mongo(
                    self.mongo.execute(&connection, action).await?,
                ))
            }),
            Command::RedisData { connection, action } => Box::pin(async move {
                Ok(Output::RedisData(
                    crate::redis_data::execute(&connection, action).await?,
                ))
            }),
            Command::BackupObjects {
                connection,
                namespace,
                schema,
            } => Box::pin(async move {
                Ok(Output::BackupObjects(
                    self.databases
                        .backup_objects_in(&connection, &namespace, schema.as_deref())
                        .await?,
                ))
            }),
            Command::BackupDatabase {
                connection,
                root,
                operation,
                control,
            } => Box::pin(async move {
                Ok(Output::BackupDatabase(
                    self.databases
                        .backup(&connection, root, operation, control)
                        .await?,
                ))
            }),
            Command::Database { connection, action } => Box::pin(async move {
                Ok(Output::Database(
                    self.databases.workbench(&connection, action).await?,
                ))
            }),
            Command::SaveQueryRows {
                connection,
                result,
                editing,
                changes,
            } => Box::pin(async move {
                let deleting = !changes.is_empty() && changes.iter().all(|change| change.delete);
                Ok(Output::SavedQueryRows {
                    result,
                    affected: self
                        .databases
                        .save_query_rows(&connection, editing, changes)
                        .await?,
                    deleting,
                })
            }),
            Command::SqlScript {
                connection,
                sql,
                mode,
            } => Box::pin(async move {
                Ok(Output::Script(Box::new(
                    self.databases.script(&connection, sql, mode).await?,
                )))
            }),
            Command::Sql { connection, sql } => Box::pin(async move {
                Ok(Output::Queries(
                    self.databases.query(&connection, &sql).await?,
                ))
            }),
            Command::Redis { connection, args } => Box::pin(async move {
                Ok(Output::Text(
                    network::redis_command(&connection, &args).await?,
                ))
            }),
            Command::Disconnect { id } => Box::pin(async move {
                self.mongo.close(&id).await;
                self.databases.disconnect(&id).await?;
                Ok(Output::Text("已断开数据库连接".into()))
            }),
            Command::DisconnectDatabase { id, database } => Box::pin(async move {
                self.databases.disconnect_database(&id, &database).await?;
                Ok(Output::Text("数据库已关闭".into()))
            }),
            Command::FileDocument { client, action } => Box::pin(async move {
                client
                    .ok_or_else(|| anyhow::anyhow!("文档必须通过原文件连接打开"))?
                    .execute(action)
                    .await
            }),
            Command::MarkdownImage { document, source } => Box::pin(async move {
                Ok(Output::MarkdownImage(
                    crate::markdown::picture(document, source).await?,
                ))
            }),
            Command::ObjectTree { connection, action } => {
                Box::pin(async move { files::object_tree::execute(&connection, action).await })
            }
            other => Box::pin(async move {
                tokio::task::spawn_blocking(move || blocking_command(other)).await?
            }),
        }
    }
}

fn blocking_command(command: Command) -> Result<Output> {
    let text = match command {
        Command::ConnectionImport { request, control } => {
            return Ok(Output::ConnectionImport(crate::connections::execute(
                request,
                control.unwrap_or_default(),
            )?));
        }
        Command::ConnectionExport {
            source,
            path,
            prepared,
            control,
        } => {
            let count = prepared.connections.len();
            let control = control.unwrap_or_default();
            let encoded = prepared.encode_checked(|| control.check())?;
            tomato_core::repository::write_export_checked(
                source.as_deref(),
                &path,
                encoded.as_bytes(),
                || control.check(),
            )?;
            format!("已导出 {count} 个连接到：{}", path.display())
        }
        Command::BackupFiles { root, request } => {
            return Ok(Output::BackupFiles(
                crate::backup::Repository::new(root).execute(request)?,
            ));
        }
        Command::NamedQueries { root, request } => {
            return Ok(Output::NamedQueries(
                crate::queries::Repository::new(root).execute(request)?,
            ));
        }
        Command::ExtractResources { options } => {
            return Ok(Output::Resources(crate::extractor::execute(options)?));
        }
        Command::Hosts {
            config_path,
            system_path,
            action,
        } => {
            return Ok(Output::Hosts(crate::hosts::execute(
                &config_path,
                &system_path,
                action,
            )?));
        }
        Command::JsonHistory { path, action } => {
            return Ok(Output::JsonHistory(crate::json_history::execute(
                &path, action,
            )?));
        }
        Command::ImportTableRows {
            path,
            definition,
            remaining,
        } => {
            return Ok(Output::StagedRows(files::import_table_rows(
                &path,
                &definition,
                remaining,
            )?));
        }
        Command::ListFiles { path } => return Ok(Output::Files(files::list(&path)?)),
        Command::Markdown { source } => {
            return Ok(Output::Markdown(crate::markdown::parse(&source)?));
        }
        Command::MarkdownHtml {
            source,
            title,
            path,
        } => {
            let path = std::path::absolute(path)?;
            crate::markdown::export_html(&source, &title, &path)?;
            format!("已导出到：{}", path.display())
        }
        Command::LocalTree { root, action } => {
            return Ok(Output::LocalTree(files::tree::execute(&root, action)?));
        }
        Command::ListVisibleFiles { path } => {
            return Ok(Output::Files(files::list_visible(&path)?));
        }
        Command::CreateDirectory { parent, name } => {
            files::create_directory(&parent, &name)?;
            return Ok(Output::Files(files::list(&parent)?));
        }
        Command::ExportCsv {
            path,
            columns,
            rows,
        } => {
            files::export_csv(&path, &columns, &rows)?;
            format!("已导出 {}", path.display())
        }
        Command::SaveDesktopShortcut {
            destination,
            shortcut,
        } => {
            shortcut.save(&destination)?;
            format!("已创建 {}", destination.display())
        }
        Command::DesktopShortcut {
            destination,
            name,
            executable,
            icon,
        } => {
            tools::desktop_shortcut(&destination, &name, &executable, &icon)?;
            format!("已创建 {}", destination.display())
        }
        Command::ReadText { path } => {
            if std::fs::metadata(&path)?.len() > 8 * 1024 * 1024 {
                anyhow::bail!("文本编辑器最多读取 8 MiB 文件");
            }
            std::fs::read_to_string(path)?
        }
        Command::WriteText { path, text } => {
            files::save_text(&path, &text)?;
            "文件已保存".into()
        }
        Command::Json { text, action } => tomato_core::tools::json_transform(&text, &action)?,
        Command::ConvertImage {
            source,
            destination,
            width,
        } => {
            tools::convert_image(&source, &destination, width)?;
            format!("已转换: {}", destination.display())
        }
        Command::MakeIcon {
            source,
            destination,
            tolerance,
            size,
        } => {
            tools::make_icon(&source, &destination, tolerance, size)?;
            format!("已生成: {}", destination.display())
        }
        Command::ConvertDataset {
            source,
            destination,
        } => format!(
            "已转换 {} 个标注文件",
            tools::voc_to_coco(&source, &destination)?
        ),
        Command::Download { url, destination } => {
            format!("下载完成: {} 字节", network::download(&url, &destination)?)
        }
        _ => anyhow::bail!("异步命令被错误分发至文件适配器"),
    };
    Ok(Output::Text(text))
}
#[cfg(test)]
mod future_memory_tests {
    use super::*;
    #[test]
    fn lightweight_commands_do_not_reserve_protocol_future_frames() {
        let mut engine = Engine::default();
        // Construct and drop futures without polling: no runtime, sockets or file writes.
        let json = engine.execute(Command::Json {
            text: "{}".into(),
            action: "format".into(),
        });
        let json_bytes = std::mem::size_of_val(json.as_ref().get_ref());
        drop(json);
        let catalog = engine.execute(Command::BackupObjects {
            connection: tomato_core::Connection::new("MYSQL"),
            namespace: "unpolled_fixture".into(),
            schema: None,
        });
        let catalog_bytes = std::mem::size_of_val(catalog.as_ref().get_ref());
        drop(catalog);
        eprintln!(
            "command future heap frame: JSON={json_bytes} bytes, database catalog={catalog_bytes} bytes"
        );
        assert!(
            json_bytes <= 2048,
            "A lightweight command reserves {json_bytes} bytes for unrelated protocol work"
        );
    }
}
