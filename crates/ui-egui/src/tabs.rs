use std::path::PathBuf;
use tomato_core::Connection;
use tomato_services::{QueryResult, terminal::Terminal};
pub mod ai_sql;
pub mod batch_edit;
pub mod cloud;
pub mod completion;
pub mod converter;
pub mod database_objects;
pub mod desktop;
pub mod downloader;
pub mod extractor;
pub mod game_dungeons;
pub mod hosts;
pub mod icon_tool;
pub mod json;
pub mod markdown;
pub mod mongo;
pub mod mq;
pub mod port_mapping;
pub mod queries;
pub mod query_results;
pub mod redis;
pub mod server;
pub mod shortcut;
pub mod sql_scope;
pub mod ssh;
pub mod table_design;
pub mod terminal;
pub mod text_state;
pub mod trae;
pub mod workbench;

pub struct Tab {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub panel: Panel,
    pub busy: bool,
    pub error: Option<String>,
}

impl Tab {
    pub fn workspace_visible(&self) -> bool {
        !self.source_id.starts_with("tool:")
            && !self.source_id.starts_with("server:")
            && !matches!(&self.panel,Panel::Editor{markdown,..}if markdown.file_embedded)
            && !matches!(&self.panel,Panel::NativeDesktop{view,..}if view.window.is_some())
    }
}
pub enum Panel {
    Cloud {
        connection: Connection,
        view: Box<cloud::State>,
    },
    MessageQueue {
        connection: Connection,
        view: Box<mq::State>,
    },
    Mongo {
        connection: std::sync::Arc<Connection>,
        view: Box<mongo::State>,
    },
    Sql {
        connection: Connection,
        sql: String,
        results: Vec<QueryResult>,
        selected: usize,
        workbench: Box<workbench::Workbench>,
    },
    Redis {
        view: Box<redis::State>,
        connection: std::sync::Arc<Connection>,
    },
    Editor {
        path: PathBuf,
        text: String,
        saved: String,
        markdown: Box<markdown::State>,
    },
    Terminal {
        connection: Connection,
        session: Terminal,
        parser: Box<vt100::Parser>,
        view: terminal::TerminalViewState,
    },
    Tool(ToolState),
    Desktop(Connection),
    NativeDesktop {
        connection: Connection,
        session: Option<tomato_services::remote_desktop::DesktopSession>,
        view: desktop::DesktopView,
    },
    Server(Box<server::ServerView>),
    Unavailable {
        kind: String,
    },
}

pub struct ToolState {
    pub converter: Option<Box<converter::State>>,
    pub port_mapping: Option<Box<port_mapping::State>>,
    pub downloader: Option<Box<downloader::State>>,
    pub(crate) music: Option<Box<crate::panels::music::State>>,
    pub game_dungeons: Option<Box<game_dungeons::State>>,
    pub shortcut: Option<Box<shortcut::State>>,
    pub icon_tool: Option<Box<icon_tool::State>>,
    pub extractor: Option<Box<extractor::State>>,
    pub trae: Option<Box<trae::State>>,
    pub hosts: Option<Box<hosts::State>>,
    pub kind: String,
    pub input: String,
    pub output: String,
    pub source: String,
    pub destination: String,
    pub color_transpose: Option<Box<crate::panels::color_transpose::State>>,
    pub history: Vec<tomato_services::json_history::Item>,
    pub history_path: PathBuf,
    pub history_selection: Option<String>,
    pub json_view: json::JsonView,
    pub json_tree: bool,
    pub json_status: json::feedback::Status,
    pub json_reset_input_cursor: bool,
    pub json_reset_output_scroll: bool,
}
impl ToolState {
    pub fn new(kind: &str) -> Self {
        Self {
            converter: None,
            port_mapping: None,
            downloader: (kind == "downloader").then(|| {
                Box::new(downloader::State::new(
                    tomato_core::Store::default_path().with_file_name("downloads.json"),
                ))
            }),
            music: None,
            game_dungeons: (kind == "game_dungeons").then(Default::default),
            shortcut: None,
            icon_tool: None,
            extractor: None,
            trae: None,
            hosts: None,
            kind: kind.into(),
            input: String::new(),
            output: String::new(),
            source: String::new(),
            destination: String::new(),
            color_transpose: None,
            history: Default::default(),
            history_path: tomato_services::json_history::legacy_path(),
            history_selection: None,
            json_view: Default::default(),
            json_tree: false,
            json_status: Default::default(),
            json_reset_input_cursor: true,
            json_reset_output_scroll: true,
        }
    }
    pub fn set_output(&mut self, value: String) {
        self.replace_output(value, false);
    }
    pub fn set_json_tree(&mut self, value: String) {
        self.replace_output(value, true);
    }
    fn replace_output(&mut self, mut value: String, tree: bool) {
        // Java TextArea removes CR when a raw transform result is assigned.
        if self.kind == "json_tool" && !tree && value.contains('\r') {
            value.retain(|ch| ch != '\r');
        }
        self.json_tree = tree;
        self.json_reset_output_scroll = true;
        if self.kind == "json_tool" {
            self.json_view.reset(&value, tree);
        }
        self.output = value;
    }
}

impl Tab {
    pub fn dirty(&self) -> bool {
        match &self.panel {
            Panel::Editor { text, saved, .. } => text != saved,
            Panel::Sql { workbench, .. } => {
                workbench.dirty()
                    || workbench
                        .designer
                        .as_ref()
                        .is_some_and(|draft| draft.has_changes())
                    || workbench
                        .document
                        .as_ref()
                        .is_some_and(|document| document.dirty())
            }
            Panel::Terminal { view, .. } => view
                .ssh
                .as_ref()
                .and_then(|w| w.editor.as_ref())
                .is_some_and(|e| e.text != e.saved || e.pending),
            _ => false,
        }
    }
}

/// Factory: one place maps the stable legacy connection type to its native UI session.
pub fn create_tab(
    connection: Connection,
    runtime: Option<&tokio::runtime::Handle>,
) -> anyhow::Result<Tab> {
    let panel = match connection.kind() {
        "ALIYUN" | "TENCENT_CLOUD" => Panel::Cloud {
            connection: connection.clone(),
            view: Box::default(),
        },
        "KAFKA" | "ROCKETMQ" => {
            let mut view = Box::<mq::State>::default();
            if connection.kind() == "KAFKA" {
                view.partitions = 3;
            }
            Panel::MessageQueue {
                connection: connection.clone(),
                view,
            }
        }
        "MYSQL" | "POSTGRESQL" | "SQLITE" | "ORACLE" | "HIVE" => Panel::Sql {
            connection: connection.clone(),
            sql: "SELECT 1 AS connected;".into(),
            results: Vec::new(),
            selected: 0,
            workbench: Box::default(),
        },
        "REDIS" => Panel::Redis {
            view: Box::new(redis::State {
                database: connection.number("redisDatabase") as u32,
                ..Default::default()
            }),
            connection: std::sync::Arc::new(connection.clone()),
        },
        "LOCAL_DIRECTORY" | "FTP" | "S3" | "ALIYUN_OSS" | "SSH" | "SFTP" | "LOCAL_TERMINAL" => {
            Panel::Terminal {
                connection: connection.clone(),
                session: Terminal::start(
                    &connection,
                    runtime.ok_or_else(|| anyhow::anyhow!("后台运行时不可用"))?,
                )?,
                view: terminal::TerminalViewState {
                    ssh: (connection.kind() != "LOCAL_TERMINAL").then(|| {
                        let mut workspace = ssh::Workspace::default();
                        if connection.kind() != "SSH" {
                            let path = if connection.kind() == "LOCAL_DIRECTORY"
                                && connection.text("directoryType") != "S3"
                            {
                                connection.text("localDirectoryPath").replace('\\', "/")
                            } else {
                                "/".into()
                            };
                            let mode = if matches!(connection.kind(), "S3" | "ALIYUN_OSS") {
                                "ICON"
                            } else {
                                "LIST"
                            };
                            workspace = ssh::Workspace::standalone_files(mode, &path);
                        }
                        Box::new(workspace)
                    }),
                    ..Default::default()
                },
                parser: Box::new(vt100::Parser::new(
                    32,
                    120,
                    tomato_core::limits::TERMINAL_SCROLLBACK,
                )),
            }
        }
        "RDP" | "VNC" | "ADB" => Panel::NativeDesktop {
            connection: connection.clone(),
            session: if connection.kind() == "RDP" && connection.text("password").is_empty() {
                None
            } else {
                let runtime = runtime.ok_or_else(|| anyhow::anyhow!("后台运行时不可用"))?;
                Some(tomato_services::remote_desktop::DesktopSession::start(
                    &connection,
                    runtime,
                )?)
            },
            view: Default::default(),
        },
        "SPICE" => Panel::Desktop(connection.clone()),
        "HTTP_SERVER" | "FTP_SERVER" | "SMB_SERVER" => {
            Panel::Server(Box::new(server::ServerView::new(
                tomato_core::server::ServerConfig::from_connection(&connection)?,
            )))
        }
        "TOOL" => {
            let mut tool = ToolState::new(connection.text("toolType"));
            if tool.kind == "music_downloader" {
                let mut state = crate::panels::music::State::default();
                state.set_directory(
                    connection
                        .text(crate::panels::music::DOWNLOAD_DIRECTORY)
                        .into(),
                );
                tool.music = Some(Box::new(state));
            }
            if let Some(state) = &mut tool.downloader {
                state.show_header = false;
            }
            if tool.kind == "port_mapping" {
                tool.port_mapping = Some(Box::new(port_mapping::State::new(
                    tomato_services::terminal::ssh::forwarding::legacy_path(),
                    runtime.cloned(),
                    tomato_core::Store::default_path().with_file_name("known_hosts"),
                )));
            }
            Panel::Tool(tool)
        }
        kind => Panel::Unavailable { kind: kind.into() },
    };
    Ok(Tab {
        id: Connection::new("TOOL").id,
        source_id: connection.id,
        title: connection.name,
        panel,
        busy: false,
        error: None,
    })
}

pub fn create_ssh_tool_tab(
    connection: Connection,
    section: tomato_services::terminal::ssh::tools::Section,
    runtime: &tokio::runtime::Handle,
) -> anyhow::Result<Tab> {
    let mut workspace = ssh::Workspace::default();
    workspace.standalone = Some(section);
    workspace.open(section);
    let session = Terminal::start_tools(&connection, runtime)?;
    Ok(Tab {
        id: Connection::new("TOOL").id,
        source_id: format!("ssh:{}:{section:?}", connection.id),
        title: format!("{} - {}", connection.name, section.label()),
        panel: Panel::Terminal {
            connection,
            session,
            parser: Box::new(vt100::Parser::new(32, 120, 0)),
            view: terminal::TerminalViewState {
                ssh: Some(Box::new(workspace)),
                ..Default::default()
            },
        },
        busy: false,
        error: None,
    })
}
