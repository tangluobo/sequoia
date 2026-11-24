mod adb_config;
#[cfg(test)]
#[path = "../../services/tests/support/adb_peer.rs"]
mod adb_peer;
mod ai_sql;
mod backup_tree;
mod backup_ui;
mod collaboration;
mod connection_edit;
mod connection_picker;
mod connections;
mod credentials;
mod data_export;
mod data_import;
mod database_admin;
mod database_objects;
mod db_tree;
mod desktops;
mod development;
mod dialogs;
mod export;
mod file_documents;
mod file_tabs;
mod import;
mod input_share;
mod local_tree;
mod markdown_html;
pub(crate) mod markdown_print;
mod modal;
mod mongo_tree;
mod picker;
mod preview_ai;
mod preview_alignment;
mod preview_binary;
mod preview_cloud_mq;
mod preview_converters;
mod preview_copy;
mod preview_designer;
mod preview_development;
mod preview_devices;
mod preview_file_drop;
mod preview_hive;
mod preview_image;
mod preview_json_selection;
mod preview_query_input;
mod preview_recovery;
mod preview_sql;
mod preview_tabs;
mod preview_tree_rename;
mod preview_workbench;
mod query_tree;
mod rdp_acceptance;
mod rdp_windows;
mod redis_tree;
mod remote_tree;
mod servers;
mod session_config;
mod settings;
mod shell;
mod shutdown;
mod sql_scope;
mod sql_tree;
mod ssh_tree;
mod tab_closes;
mod tab_strip;
mod table_config;
mod table_design;
mod table_transfer;
mod terminal_config;
#[cfg(test)]
pub(crate) mod tests;
mod tool_sessions;
mod tree_drag;
mod tree_keyboard;
mod tree_objects;
mod tree_rename;
mod workspace;

use crate::{
    panels::{self, Intent},
    tabs::{self, Panel, Tab, ToolState},
    tasks::TaskQueue,
};
use egui::{Color32, RichText, ScrollArea, TextEdit, Ui};
use std::{collections::HashMap, path::PathBuf, sync::mpsc};
use tomato_core::{
    Connection, Store,
    catalog::{DATABASE_PROFILES, KINDS, TOOLS},
    repository::{ConnectionFileRepository, WorkspaceRepository},
};
use tomato_services::{
    commands::{Command, Output},
    network::RunningServer,
};

struct Pending {
    tab_id: String,
    purpose: Purpose,
}
enum Purpose {
    ObjectHeaders(database_objects::loading::Stamp),
    ObjectFields(database_objects::loading::Stamp),
    DesignSql(u64),
    ConnectionExport,
    ConnectionImportRead,
    ConnectionImportCommit,
    RemoteTree {
        connection: String,
        generation: String,
        parent: Option<remote_tree::Entry>,
    },
    TreeObjects(Box<tree_objects::Dialog>),
    ObjectMutation(Box<database_objects::MutationJob>),
    SqlDisconnect(String),
    DatabaseAdmin(String),
    TreeTables {
        connection: Connection,
        schema: String,
        epoch: String,
        expected_tables: Option<std::sync::Arc<Vec<tomato_core::workbench::TableInfo>>>,
    },
    TableTransfer(String, u64, table_transfer::Job),
    TablePaste(Box<table_transfer::Refresh>),
    PgTree(sql_tree::postgres::Job),
    DirectTree(sql_tree::direct::Job),
    BackupTree(backup_tree::Job),
    Backup(String, backup_ui::Job),
    NamedQuery(query_tree::Job),
    RedisTree {
        connection: String,
        generation: String,
    },
    MongoTree {
        connection: String,
        generation: String,
        target: Option<tomato_services::mongo::Target>,
        dialog: Option<String>,
    },
    LocalTree {
        connection: String,
        path: PathBuf,
        generation: u64,
    },
    ToolConversion,
    Certificate,
    CertificateBuckets,
    Normal,
    QuerySave {
        result: usize,
        generation: u64,
    },
    Markdown(u64),
    MarkdownHtml,
    SqlCompletion(u64),
    SqlDatabases {
        connection: String,
        generation: u64,
        preferred: Option<String>,
    },
    SqlRequest(tabs::sql_scope::ResultContext),
    AiSql(u64),
    MarkdownImage(String, u64),
    LocalMutation {
        connection: Connection,
    },
    Json(crate::tabs::json::feedback::Feedback),
    Read,
    SqlRead,
    Save(String),
}
struct ConnectionDialog {
    draft: Connection,
    adb_port: String,
    adb_pairing: adb_config::Pairing,
    tab: usize,
    editing: bool,
}
enum Confirmation {
    Delete(Vec<String>),
    Close(String),
    Quit,
}

type SettingsState = tomato_core::preferences::Preferences;
struct LayoutMetrics {
    nav: egui::Rect,
    sidebar: egui::Rect,
    header: egui::Rect,
}
impl Default for LayoutMetrics {
    fn default() -> Self {
        Self {
            nav: egui::Rect::NOTHING,
            sidebar: egui::Rect::NOTHING,
            header: egui::Rect::NOTHING,
        }
    }
}

pub struct TomatoApp {
    input_share: Option<Box<input_share::State>>,
    collaboration: Option<Box<collaboration::State>>,
    development: Option<Box<crate::development::State>>,
    remote_hosts: HashMap<String, remote_tree::Host>,
    instance_properties: Option<remote_tree::Properties>,
    markdown_print: Option<markdown_print::State>,
    markdown_html: Option<markdown_html::State>,
    _ddns: Option<tomato_services::cloud::ddns::Runner>,
    certificate_renewals: Option<tomato_services::cloud::certificate::renewal::Runner>,
    context: egui::Context,
    repository: Box<dyn WorkspaceRepository>,
    store: Store,
    runtime: Option<tokio::runtime::Runtime>,
    shutdown: Option<shutdown::Shutdown>,
    queue: TaskQueue,
    pending: HashMap<u64, Pending>,
    tabs: Vec<Tab>,
    closing_tabs: std::collections::VecDeque<String>,
    active: Option<String>,
    filter: String,
    section: usize,
    module_active: [Option<String>; 5],
    picker: Option<usize>,
    picker_parent: Option<String>,
    icons: crate::icons::Icons,
    layout: LayoutMetrics,
    settings: SettingsState,
    settings_page: settings::Page,
    settings_form: Option<Box<settings::Form>>,
    table_config: Option<Box<table_config::Dialog>>,
    download_manager: std::sync::Weak<tomato_services::downloader::Manager>,
    ai_dialog: Option<ai_sql::Dialog>,
    backup_dialog: Option<backup_ui::Draft>,
    data_export: Option<data_export::Draft>,
    data_import: Option<data_import::Draft>,
    table_transfer: Option<table_transfer::Draft>,
    table_paste: Option<tomato_services::backup::Control>,
    table_paste_notice: Option<table_transfer::PasteNotice>,
    database_admin: Option<database_admin::Draft>,
    session_config: Option<session_config::Draft>,
    tree_objects: Option<Box<tree_objects::Dialog>>,
    sql_selection: std::collections::BTreeSet<String>,
    sql_anchor: Option<String>,
    table_clipboard_port: Box<dyn tomato_services::database::transfer::clipboard::Repository>,
    backups: backup_tree::Cache,
    backup_edit: Option<backup_tree::Dialog>,
    preferences: tomato_core::preferences::PreferencesRepository,
    selected_connection: Option<String>,
    connection_selection: std::collections::BTreeSet<String>,
    connection_anchor: Option<String>,
    tree_keyboard_anchor: Option<String>,
    file_owner_close: Option<String>,
    connection_rename: Option<connection_edit::Rename>,
    tree_rename: tree_rename::State,
    collapsed: std::collections::HashSet<String>,
    ssh_hosts: HashMap<String, ssh_tree::Host>,
    local_trees: HashMap<String, local_tree::Host>,
    mongo_hosts: HashMap<String, mongo_tree::Host>,
    redis_hosts: HashMap<String, redis_tree::Host>,
    sql_hosts: HashMap<String, sql_tree::Host>,
    queries: query_tree::Controller,
    mongo_dialog: Option<mongo_tree::Dialog>,
    local_edit: Option<local_tree::Edit>,
    local_selection: std::collections::BTreeSet<(String, PathBuf)>,
    local_anchor: Option<String>,
    dialog: Option<ConnectionDialog>,
    import_dialog: Option<import::Dialog>,
    import_reader: Option<import::Reader>,
    export_dialog: Option<export::Dialog>,
    credential: Option<credentials::Prompt>,
    confirmation: Option<Confirmation>,
    status: String,
    error: Option<String>,
    load_failed: bool,
    quit_approved: bool,
    server_sender: mpsc::Sender<(String, anyhow::Result<RunningServer>)>,
    server_receiver: mpsc::Receiver<(String, anyhow::Result<RunningServer>)>,
}

impl TomatoApp {
    pub fn new(ctx: &egui::Context, runtime: tokio::runtime::Runtime) -> Self {
        tomato_services::init_crypto();
        let mut app = Self::with_repository(
            ctx,
            runtime,
            ConnectionFileRepository::new(Store::default_path()),
        );
        if std::env::var_os("TOMATO_UI_CAPTURE").is_some() {
            match std::env::var("TOMATO_UI_PREVIEW").as_deref() {
                #[cfg(windows)]
                Ok("markdown_print") => {
                    if let Err(error) = app.preview_markdown_print() {
                        app.error = Some(format!("打印验收夹具错误：{error:#}"));
                    }
                }
                #[cfg(windows)]
                Ok("markdown_html") => {
                    if let Err(error) = app.preview_markdown_html() {
                        app.error = Some(format!("HTML 导出验收夹具错误：{error:#}"));
                    }
                }
                Ok("json_selection") => {
                    if let Err(error) = app.preview_json_selection() {
                        app.error = Some(format!("JSON 验收夹具错误：{error:#}"));
                    }
                }
                Ok("image_format_converter" | "dataset_converter") => {
                    let kind = std::env::var("TOMATO_UI_PREVIEW").unwrap_or_default();
                    if let Err(error) = app.preview_converter(&kind) {
                        app.error = Some(format!("转换验收夹具错误：{error:#}"));
                    }
                }
                Ok("hive_designer") => {
                    if let Err(error) = app.preview_hive_designer() {
                        app.error = Some(format!("Hive 验收夹具错误：{error:#}"));
                    }
                }
                Ok("table_config") => {
                    app.table_config = Some(Box::new(table_config::Dialog::new(
                        app.settings.table_font(),
                    )));
                }
                Ok(
                    "sql_results"
                    | "sql_results_footer"
                    | "sql_results_filter"
                    | "sql_results_date"
                    | "sql_results_time"
                    | "sql_results_datetime",
                ) => {
                    if let Err(error) = app.preview_sql_results(ctx) {
                        app.error = Some(format!("SQL 结果验收夹具错误：{error:#}"));
                    }
                }
                Ok(
                    kind @ ("cloud_dns" | "cloud_certificate" | "kafka_topics" | "kafka_send"
                    | "rocketmq_topics" | "rocketmq_send" | "kafka_groups"
                    | "rocketmq_groups" | "kafka_batch" | "rocketmq_batch"),
                ) => {
                    if let Err(error) = app.preview_cloud_mq(kind) {
                        app.error = Some(format!("云/MQ 验收夹具错误：{error:#}"));
                    }
                }
                Ok(
                    kind @ ("adb" | "adb_config" | "adb_png" | "adb_window" | "port_mapping"
                    | "port_mapping_edit"),
                ) => {
                    if let Err(error) = app.preview_device(kind) {
                        app.error = Some(format!("设备验收夹具错误：{error:#}"));
                    }
                }
                Ok("layout_alignment") => {
                    if let Err(error) = app.preview_alignment(ctx) {
                        app.error = Some(format!("布局验收夹具错误：{error:#}"));
                    }
                }
                Ok("tab_strip") => {
                    if let Err(error) = app.preview_tabs(ctx) {
                        app.error = Some(format!("标签验收夹具错误：{error:#}"));
                    }
                }
                Ok("database_admin") => {
                    if let Err(error) = app.preview_database_admin() {
                        app.error = Some(format!("数据库对话框夹具错误: {error:#}"));
                    }
                }
                Ok("table_transfer") => {
                    if let Err(error) = app.preview_table_transfer() {
                        app.error = Some(format!("表传输夹具错误: {error:#}"));
                    }
                }
                Ok("table_designer") => {
                    if let Err(error) = app.preview_table_designer() {
                        app.error = Some(format!("表设计器夹具错误: {error:#}"));
                    }
                }
                Ok("binary_table") => {
                    if let Err(error) = app.preview_binary_table() {
                        app.error = Some(format!("二进制表验收夹具错误：{error:#}"));
                    }
                }
                Ok("sql_workbench" | "table_data") => {
                    if let Err(error) = app.preview_sql_workbench() {
                        app.error = Some(format!("SQL 工作台夹具错误：{error:#}"));
                    }
                }
                Ok(
                    "backup" | "restore" | "restore_confirm" | "backup_complete" | "backup_error",
                ) => {
                    if let Err(error) = app.preview_backup() {
                        app.error = Some(format!("备份夹具错误：{error:#}"));
                    }
                }
                Ok("query_dialog") => {
                    if let Err(error) = app.preview_query_dialog(ctx) {
                        app.error = Some(format!("查询弹窗验收夹具错误：{error:#}"));
                    }
                }
                Ok("tree_rename") => {
                    if let Err(error) = app.preview_tree_rename(ctx) {
                        app.error = Some(format!("连接树验收夹具错误：{error:#}"));
                    }
                }
                Ok("image_preview") => {
                    if let Err(error) = app.preview_image(ctx) {
                        app.error = Some(format!("图片预览验收夹具错误：{error:#}"));
                    }
                }
                Ok("sql_completion") => {
                    if let Err(error) = app.preview_sql_completion(ctx) {
                        app.error = Some(format!("SQL 补全验收夹具错误：{error:#}"));
                    }
                }
                Ok("database_recovery") => {
                    if let Err(error) = app.preview_database_recovery(ctx) {
                        app.error = Some(format!("数据库恢复验收失败：{error:#}"));
                    }
                }
                Ok("query_input") => {
                    if let Err(error) = app.preview_query_input(ctx) {
                        app.error = Some(format!("查询输入验收错误：{error:#}"));
                    }
                }
                Ok("data_export") => {
                    if let Err(error) = app.preview_data_export(ctx) {
                        app.error = Some(format!("导出验收夹具错误：{error:#}"));
                    }
                }
                Ok("data_import") => {
                    if let Err(error) = app.preview_data_import(ctx) {
                        app.error = Some(format!("导入验收夹具错误：{error:#}"));
                    }
                }
                Ok("collaboration") => {
                    if let Err(error) = app.preview_collaboration(ctx) {
                        app.error = Some(format!("云协作验收夹具错误：{error:#}"));
                    }
                }
                Ok("input_share" | "input_share_connected") => {
                    if let Some(runtime) = &app.runtime {
                        let mut state = input_share::State::new(runtime, ctx);
                        state.preview(
                            std::env::var("TOMATO_UI_PREVIEW").as_deref()
                                == Ok("input_share_connected"),
                        );
                        app.input_share = Some(Box::new(state));
                    }
                }
                Ok("file_drop") => {
                    if let Err(error) = app.preview_file_drop(ctx) {
                        app.error = Some(format!("文件拖放验收错误：{error:#}"));
                    }
                }
                Ok("ai_sql_prompt" | "ai_sql_settings") => {
                    if let Err(error) = app.preview_ai_sql(ctx) {
                        app.error = Some(format!("AI SQL 夹具错误：{error:#}"));
                    }
                }
                Ok("object_copy") => {
                    if let Err(error) = app.preview_copy(ctx) {
                        app.error = Some(format!("对象复制验收夹具错误：{error:#}"));
                    }
                }
                Ok("mongo") => {
                    if let Some(connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| {
                            c.kind() == "MONGODB"
                                && c.name == "MongoDB 验收"
                                && c.text("host") == "127.0.0.1"
                        })
                        .cloned()
                        && let Ok(target) = std::env::var("TOMATO_UI_MONGO_TARGET")
                            .ok()
                            .and_then(|s| {
                                serde_json::from_str::<tomato_services::mongo::Target>(&s).ok()
                            })
                            .ok_or(())
                        && target.database.starts_with("tomato_fixture")
                    {
                        app.open_connection(connection.clone());
                        app.mongo_collection(connection, target);
                        if let Some(tab) = app.tabs.last_mut()
                            && let Panel::Mongo { view, .. } = &mut tab.panel
                        {
                            view.page = std::env::var("TOMATO_UI_MONGO_PAGE")
                                .ok()
                                .and_then(|s| s.parse::<usize>().ok())
                                .unwrap_or(0)
                                .min(3);
                        }
                    }
                }
                Ok("extractor") => {
                    if let Some(source) =
                        std::env::var_os("TOMATO_UI_PREVIEW_PATH").map(PathBuf::from)
                        && source
                            .file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("tomato_fixture"))
                    {
                        app.section = 1;
                        app.open_tool("resource_extractor", "资源图标提取");
                        if let Some(tab) = app.tabs.last_mut()
                            && let Panel::Tool(tool) = &mut tab.panel
                        {
                            tool.source = source.to_string_lossy().into_owned();
                            tool.extractor.get_or_insert_with(Default::default).status =
                                "正在运行...".into();
                            let id = tab.id.clone();
                            app.submit(
                                &id,
                                Command::ExtractResources {
                                    options: tomato_services::extractor::Options {
                                        source,
                                        ..Default::default()
                                    },
                                },
                                Purpose::Normal,
                            );
                        }
                    }
                }
                Ok("trae") => {
                    if let Some(root) =
                        std::env::var_os("TOMATO_UI_PREVIEW_PATH").map(PathBuf::from)
                    {
                        app.section = 1;
                        app.open_tool("trae_session", "Trae 会话");
                        if let Some(tab) = app.tabs.last_mut()
                            && let Panel::Tool(tool) = &mut tab.panel
                        {
                            tool.trae.get_or_insert_with(Default::default).path =
                                Some(root.join("trae_sessions.json"));
                        }
                    }
                }
                Ok("hosts") => {
                    if let Some(root) =
                        std::env::var_os("TOMATO_UI_PREVIEW_PATH").map(PathBuf::from)
                    {
                        let paths = (root.join("hosts_groups.json"), root.join("hosts"));
                        if let Ok(snapshot) = tomato_services::hosts::execute(
                            &paths.0,
                            &paths.1,
                            tomato_services::hosts::Action::Load,
                        ) {
                            app.section = 1;
                            app.open_tool("hosts_file", "Hosts 文件");
                            if let Some(tab) = app.tabs.last_mut()
                                && let Panel::Tool(tool) = &mut tab.panel
                            {
                                let view = tool.hosts.get_or_insert_with(Default::default);
                                view.paths = Some(paths);
                                view.selected = Some(0);
                                view.accept(snapshot);
                                view.status.clear();
                                if std::env::var("TOMATO_UI_PREVIEW_DIALOG").as_deref()
                                    == Ok("hosts-sudo")
                                {
                                    let snapshot = view.snapshot.as_ref().unwrap();
                                    let request = tomato_services::hosts::SaveRequest {
                                        previous: Box::new(snapshot.as_ref().clone()),
                                        config: snapshot.config.clone(),
                                        system: Some(view.text.clone()),
                                        apply: false,
                                    };
                                    view.fail(
                                        ctx,
                                        &tomato_services::hosts::ElevationRequired(Box::new(
                                            request,
                                        ))
                                        .into(),
                                    );
                                }
                            }
                        }
                    }
                }
                Ok("redis") => {
                    if let Some(connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| c.kind() == "REDIS" && c.name == "Redis 验收")
                        .cloned()
                    {
                        let id = connection.id.clone();
                        let database = connection.number("redisDatabase") as u32;
                        app.open_connection(connection);
                        app.redis_database(&id, database);
                        if std::env::var("TOMATO_UI_PREVIEW_DIALOG").as_deref() == Ok("redis-value")
                            && let Some(tab) = app.tabs.last_mut()
                            && let Panel::Redis { view, .. } = &mut tab.panel
                        {
                            view.followup =
                                Some(tomato_services::redis_data::Action::Read { key: "a".into() });
                        }
                    }
                }
                Ok("markdown" | "markdown_find") => {
                    if let Some(path) = std::env::var_os("TOMATO_UI_PREVIEW_PATH") {
                        app.open_editor(PathBuf::from(path));
                        if std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("markdown_find")
                            && let Some(tab) = app.tabs.last_mut()
                            && let Panel::Editor { markdown, .. } = &mut tab.panel
                        {
                            markdown.find = true;
                            markdown.replace_visible = true;
                            if let Ok(expression) = std::env::var("TOMATO_UI_PREVIEW_REGEX") {
                                markdown.needle = expression;
                                markdown.regex = true;
                            } else {
                                markdown.needle = "中文".into();
                            }
                        }
                    }
                }
                Ok("downloader_saved") => {
                    if let Err(error) = app.preview_saved_downloader(ctx) {
                        app.error = Some(format!("下载器验收夹具错误：{error:#}"));
                    }
                }
                Ok("desktop_shortcut" | "icon_tool" | "downloader") => {
                    app.section = 1;
                    let preview = std::env::var("TOMATO_UI_PREVIEW").unwrap_or_default();
                    let (kind, title) = match preview.as_str() {
                        "desktop_shortcut" => ("desktop_shortcut", "桌面快捷方式"),
                        "icon_tool" => ("image_background_remover", "图标制作工具"),
                        _ => ("downloader", "下载器"),
                    };
                    app.open_tool(kind, title);
                    if let Some(tab) = app.tabs.last_mut()
                        && let Panel::Tool(tool) = &mut tab.panel
                    {
                        if kind == "desktop_shortcut" {
                            tool.shortcut.get_or_insert_with(Default::default).tab =
                                std::env::var("TOMATO_UI_PREVIEW_TAB")
                                    .ok()
                                    .and_then(|s| s.parse().ok())
                                    .unwrap_or(0)
                                    .min(2);
                        }
                        if kind == "image_background_remover"
                            && let Some(path) = std::env::var_os("TOMATO_UI_PREVIEW_PATH")
                        {
                            tool.source = PathBuf::from(path).display().to_string();
                            let state = tool.icon_tool.get_or_insert_with(Default::default);
                            state.invalidate();
                        }
                        if kind == "downloader"
                            && let Some(state) = tool.downloader.as_mut()
                        {
                            match std::env::var("TOMATO_UI_PREVIEW_DIALOG").as_deref() {
                                Ok("add") => state.open(crate::tabs::downloader::Kind::Add),
                                Ok("settings") => {
                                    state.open(crate::tabs::downloader::Kind::Settings)
                                }
                                _ => {}
                            }
                        }
                    }
                }
                Ok("json") => {
                    app.section = 1;
                    app.open_tool("json_tool", "JSON处理工具");
                    if let Some(tab) = app.tabs.last_mut()
                        && let Panel::Tool(tool) = &mut tab.panel
                    {
                        tool.input = r#"{"项目":"Tomato","连接":[{"类型":"SSH","端口":22},{"类型":"S3","Bucket":"示例"}],"空对象":{},"空数组":[]}"#.into();
                        if let Ok(formatted) =
                            tomato_core::tools::json_transform(&tool.input, "format")
                        {
                            tool.set_json_tree(formatted);
                            tool.json_status.success("格式化成功！");
                        }
                    }
                }
                Ok("server") => {
                    if let Some(connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| c.kind() == "HTTP_SERVER")
                        .cloned()
                    {
                        app.open_connection(connection);
                    }
                }
                Ok("files") => {
                    if let Some(connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| c.kind() == "LOCAL_DIRECTORY" && c.text("directoryType") != "S3")
                        .cloned()
                    {
                        app.open_connection(connection);
                    }
                }
                Ok("import") => {
                    let input = r#"[{"id":"group","name":"项目目录"},{"id":"a","name":"Linux 测试服务器","type":"SSH","parentId":"group"},{"id":"b","name":"MySQL 测试数据库","type":"MYSQL","parentId":"group"},{"id":"orphan","name":"外部目录中的连接","type":"SSH","parentId":"external"}]"#;
                    if let Ok(mut existing) = tomato_core::legacy::decode(input) {
                        existing.retain(|c| c.id == "a");
                        let store = Store {
                            connections: existing,
                            ..Default::default()
                        };
                        if let Ok(plan) = tomato_core::config::ImportPlan::parse(input, &store) {
                            app.import_dialog = Some(import::Dialog::new(plan));
                        }
                    }
                }
                Ok("export") => {
                    if let Err(error) = app.preview_connection_export() {
                        app.error = Some(format!("导出连接夹具错误：{error:#}"));
                    }
                }
                Ok("object_tree") => {
                    if let Some(connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| {
                            tomato_services::files::object_tree::is_object(c)
                                && c.text("username") == "fixture-key"
                                && c.text("endpoint").starts_with("http://127.0.0.1:")
                        })
                        .cloned()
                    {
                        app.open_connection(connection.clone());
                        app.open_tree_editor(connection, PathBuf::from("/README.md"));
                    }
                }
                Ok("types") => app.picker = Some(0),
                Ok("tools") => app.section = 1,
                Ok("game_dungeons") => {
                    app.section = 1;
                    app.open_tool("game_dungeons", "游戏副本管理");
                }
                Ok("ssh") => {
                    if let Some(c) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| {
                            c.kind() == "SSH"
                                && c.text("host") == "127.0.0.1"
                                && c.text("username") == "fixture"
                        })
                        .cloned()
                    {
                        app.ssh_hosts.entry(c.id.clone()).or_default();
                        app.open_connection(c);
                        if let Some(tab) = app.tabs.last_mut()
                            && let Panel::Terminal { view, .. } = &mut tab.panel
                            && let Some(w) = &mut view.ssh
                        {
                            for s in [
                                tomato_services::terminal::ssh::tools::Section::Files,
                                tomato_services::terminal::ssh::tools::Section::Monitor,
                                tomato_services::terminal::ssh::tools::Section::Ports,
                                tomato_services::terminal::ssh::tools::Section::Docker,
                            ] {
                                w.open(s);
                            }
                        }
                    }
                    app.terminal_config_fixture();
                }
                Ok("vnc") => {
                    if let Some(connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| {
                            c.kind() == "VNC"
                                && c.text("host") == "127.0.0.1"
                                && c.text("username") == "fixture"
                        })
                        .cloned()
                    {
                        app.open_connection(connection);
                    }
                }
                Ok("rdp") => {
                    let saved_target = rdp_acceptance::saved_target();
                    if let Some(mut connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| {
                            c.kind() == "RDP"
                                && if let Some((id, _)) = saved_target {
                                    c.id == id
                                } else {
                                    c.text("host") == "127.0.0.1" && c.text("username") == "fixture"
                                }
                        })
                        .cloned()
                    {
                        if saved_target.is_some() {
                            if let Ok(mode) = std::env::var("TOMATO_RDP_CAPTURE_MODE")
                                && matches!(mode.as_str(), "TAB" | "WINDOW")
                            {
                                connection.set("rdpOpenMode", mode);
                            }
                            connection.set("fullscreen", false);
                        }
                        app.open_connection(connection);
                    }
                }
                Ok("database") => {
                    if let Some(connection) = app
                        .store
                        .connections
                        .iter()
                        .find(|c| c.kind() == "SQLITE")
                        .cloned()
                    {
                        app.open_connection(connection.clone());
                        app.open_table(
                            connection,
                            tomato_core::workbench::TableInfo {
                                table: tomato_core::workbench::TableId {
                                    schema: "main".into(),
                                    name: "orders".into(),
                                },
                                view: false,
                            },
                            false,
                        );
                    }
                }
                _ => {}
            }
        }
        app
    }

    fn with_repository(
        ctx: &egui::Context,
        runtime: tokio::runtime::Runtime,
        repository: impl WorkspaceRepository + 'static,
    ) -> Self {
        let startup =
            std::env::var_os("TOMATO_RENDER_DIAGNOSTICS").map(|_| std::time::Instant::now());
        let mark = |phase: &str| {
            if let Some(startup) = startup {
                eprintln!(
                    "Startup {phase}: {:.3}ms",
                    startup.elapsed().as_secs_f64() * 1000.
                );
            }
        };
        load_fonts(ctx);
        mark("fonts installed");
        let (store, error, load_failed) = match repository.load() {
            Ok(store) => (store, None, false),
            Err(error) => (
                Store::default(),
                Some(format!("配置加载失败，已禁用保存以保护原文件：{error:#}")),
                true,
            ),
        };
        let preferences = tomato_core::preferences::PreferencesRepository::new(
            repository
                .location()
                .map(|p| p.with_file_name("global.json"))
                .unwrap_or_else(|| Store::default_path().with_file_name("global.json")),
        );
        mark("connections loaded");
        let (settings, preferences_error) = match preferences.load() {
            Ok(s) => (s, None),
            Err(e) => (
                SettingsState::default(),
                Some(format!("全局配置加载失败：{e:#}")),
            ),
        };
        let error = error.or(preferences_error);
        crate::font_assets::terminal::ensure(ctx, &settings.ssh_terminal_font_name);
        crate::theme::install(ctx);
        if settings::grayscale_text(&settings) {
            crate::font_assets::set_grayscale(ctx, true);
        }
        mark("preferences and theme installed");
        let queue = TaskQueue::new(&runtime, ctx.clone());
        crate::tabs::markdown::search_job::install(ctx, runtime.handle());
        #[cfg(not(test))]
        let ddns = repository.location().map(|path| {
            tomato_services::cloud::ddns::Runner::start(
                path.with_file_name("ddns.json"),
                path.to_owned(),
                runtime.handle(),
            )
        });
        #[cfg(test)]
        let ddns = None;
        #[cfg(not(test))]
        let certificate_renewals = repository.location().map(|path| {
            let context = ctx.clone();
            tomato_services::cloud::certificate::renewal::Runner::start(
                path.to_owned(),
                runtime.handle(),
                move || context.request_repaint(),
            )
        });
        #[cfg(test)]
        let certificate_renewals = None;
        let (server_sender, server_receiver) = mpsc::channel();
        let collapsed = store.connections.iter().map(|c| c.id.clone()).collect();
        Self {
            development: None,
            input_share: None,
            collaboration: None,
            remote_hosts: Default::default(),
            instance_properties: None,
            _ddns: ddns,
            certificate_renewals,
            context: ctx.clone(),
            repository: Box::new(repository),
            store,
            runtime: Some(runtime),
            markdown_print: None,
            markdown_html: None,
            shutdown: None,
            queue,
            pending: HashMap::new(),
            tabs: Vec::new(),
            closing_tabs: Default::default(),
            active: None,
            filter: String::new(),
            section: 0,
            module_active: Default::default(),
            picker: None,
            picker_parent: None,
            icons: Default::default(),
            layout: Default::default(),
            settings,
            settings_page: Default::default(),
            settings_form: None,
            table_config: None,
            download_manager: Default::default(),
            ai_dialog: None,
            backup_dialog: None,
            data_export: None,
            data_import: None,
            table_transfer: None,
            table_paste: None,
            table_paste_notice: None,
            database_admin: None,
            session_config: None,
            tree_objects: None,
            sql_selection: Default::default(),
            sql_anchor: None,
            table_clipboard_port: {
                #[cfg(test)]
                {
                    Box::<tomato_services::database::transfer::clipboard::Memory>::default()
                }
                #[cfg(not(test))]
                {
                    Box::new(tomato_services::database::transfer::clipboard::System)
                }
            },
            backups: Default::default(),
            backup_edit: None,
            preferences,
            selected_connection: None,
            connection_selection: Default::default(),
            connection_anchor: None,
            tree_keyboard_anchor: None,
            file_owner_close: None,
            connection_rename: None,
            tree_rename: None,
            collapsed,
            ssh_hosts: Default::default(),
            local_trees: Default::default(),
            mongo_hosts: Default::default(),
            redis_hosts: Default::default(),
            sql_hosts: Default::default(),
            queries: Default::default(),
            mongo_dialog: None,
            local_edit: None,
            local_selection: Default::default(),
            local_anchor: None,
            dialog: None,
            import_dialog: None,
            import_reader: None,
            export_dialog: None,
            credential: None,
            confirmation: None,
            status: "就绪".into(),
            error,
            load_failed,
            quit_approved: false,
            server_sender,
            server_receiver,
        }
    }

    fn active_source(&self) -> Option<&str> {
        self.tabs
            .iter()
            .find(|tab| Some(&tab.id) == self.active.as_ref())
            .map(|tab| tab.source_id.as_str())
    }
}

impl Drop for TomatoApp {
    fn drop(&mut self) {
        let trace = std::env::var_os("TOMATO_CAPTURE_TRACE").is_some();
        let started = std::time::Instant::now();
        if trace {
            eprintln!("Application drop: releasing {} tabs", self.tabs.len());
        }
        self.tabs.clear();
        if trace {
            eprintln!(
                "Application drop: tabs released after {:?}",
                started.elapsed()
            );
        }
        if let Some(runtime) = self.runtime.take() {
            // Blocking HTTP jobs must not keep a closed desktop window alive for an hour.
            runtime.shutdown_timeout(std::time::Duration::from_millis(
                if self.shutdown.is_some() { 100 } else { 1000 },
            ));
        }
        if trace {
            eprintln!(
                "Application drop: runtime released after {:?}",
                started.elapsed()
            );
        }
    }
}

fn load_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for path in [
        "C:/Windows/Fonts/msyh.ttc",
        "C:/Windows/Fonts/simhei.ttf",
        "/System/Library/Fonts/PingFang.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    ] {
        if let Ok(data) = std::fs::read(path) {
            let mut data = egui::FontData::from_owned(data);
            // JavaFX System resolves to Microsoft YaHei UI on Chinese Windows:
            // it is face 1 of msyh.ttc, while face 0 is Microsoft YaHei.
            if path == "C:/Windows/Fonts/msyh.ttc"
                && data
                    .font
                    .get(8..12)
                    .is_some_and(|count| count == [0, 0, 0, 2])
            {
                data.index = 1;
            }
            fonts.font_data.insert("cjk".into(), data.into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            break;
        }
    }
    if let Ok(data) = std::fs::read("C:/Windows/Fonts/segoeui.ttf") {
        fonts
            .font_data
            .insert("system-ui".into(), egui::FontData::from_owned(data).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "system-ui".into());
    }
    if let Ok(data) = std::fs::read("C:/Windows/Fonts/consola.ttf") {
        fonts.font_data.insert(
            "system-monospace".into(),
            egui::FontData::from_owned(data).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "system-monospace".into());
    }
    // Match JavaFX's Windows symbol fallback, including the node clipboard glyph.
    // Read the installed font locally; it is not bundled in portable packages.
    if let Ok(data) = std::fs::read("C:/Windows/Fonts/seguisym.ttf") {
        fonts.font_data.insert(
            "system-symbols".into(),
            egui::FontData::from_owned(data).into(),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            let names = fonts.families.entry(family).or_default();
            names.insert(1.min(names.len()), "system-symbols".into());
        }
    }
    let bold = if let Ok(data) = std::fs::read("C:/Windows/Fonts/segoeuib.ttf") {
        fonts.font_data.insert(
            "md-bold-data".into(),
            egui::FontData::from_owned(data).into(),
        );
        let mut fallback = fonts.families[&egui::FontFamily::Proportional].clone();
        fallback.insert(0, "md-bold-data".into());
        fallback
    } else {
        fonts.families[&egui::FontFamily::Proportional].clone()
    };
    fonts
        .families
        .insert(egui::FontFamily::Name("md-bold".into()), bold.clone());
    fonts
        .families
        .insert(egui::FontFamily::Name("java-ui-bold".into()), bold);
    fonts.font_data.insert(
        "fontawesome-data".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/fontawesome.ttf")).into(),
    );
    fonts.families.insert(
        egui::FontFamily::Name("fontawesome".into()),
        vec!["fontawesome-data".into()],
    );
    let mut table = fonts.families[&egui::FontFamily::Proportional].clone();
    if cfg!(windows) && fonts.font_data.contains_key("cjk") {
        table.retain(|name| name != "cjk");
        table.insert(0, "cjk".into());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("java-ui".into()), table.clone());
    // JavaFX's Windows UI font covers both Latin and Chinese. Mixing Segoe UI
    // with a CJK fallback gives adjacent labels/editors different line metrics.
    // Reuse the same loaded font names for ordinary controls as for native tabs.
    if cfg!(windows) && fonts.font_data.contains_key("cjk") {
        fonts
            .families
            .insert(egui::FontFamily::Proportional, table.clone());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("table".into()), table.clone());
    if fonts.font_data.contains_key("md-bold-data") {
        table.insert(0, "md-bold-data".into());
    }
    fonts
        .families
        .insert(egui::FontFamily::Name("table-bold".into()), table);
    crate::font_assets::install(ctx, fonts);
}
