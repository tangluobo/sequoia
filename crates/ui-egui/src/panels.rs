mod cloud;
pub(crate) mod color_transpose;
mod converters;
mod copy_ui;
mod desktop;
mod downloader;
mod editor;
#[cfg(windows)]
pub(crate) use editor::print_document;
mod extractor;
mod game_dungeons;
mod hosts;
mod icon_tool;
mod image_ui;
mod json;
mod mongo;
mod mq;
pub(crate) mod music;
mod port_mapping;
mod redis;
mod remote_table;
mod server;
mod shortcut;
mod sql;
pub(crate) mod terminal;
mod tools;
mod trae;
mod upload_ui;
use crate::tabs::{Panel, Tab, ToolState};
use egui::{Color32, RichText, ScrollArea, TextEdit, Ui};
use std::path::PathBuf;
use tomato_services::commands::Command;
use tools::tool_panel;
pub(crate) fn ensure_bold_font(ctx: &egui::Context) {
    editor::ensure_bold_cjk(ctx);
}
pub(crate) fn render_query_results(
    ui: &mut Ui,
    result: &tomato_services::QueryResult,
    state: &mut crate::tabs::query_results::State,
    id: egui::Id,
) {
    sql::render_results(ui, result, state, id);
}
pub(crate) fn render_query_results_footer(
    ui: &mut Ui,
    result: &tomato_services::QueryResult,
    state: &mut crate::tabs::query_results::State,
    connection: &tomato_core::Connection,
    id: egui::Id,
) {
    sql::render_results_footer(ui, result, state, connection, id);
}
pub fn copy_dialogs(ctx: &egui::Context, workspace: &mut crate::tabs::ssh::Workspace) {
    copy_ui::render(ctx, workspace);
    upload_ui::render(ctx, workspace);
}
pub fn image_dialogs(ctx: &egui::Context, workspace: &mut crate::tabs::ssh::Workspace) {
    image_ui::render(ctx, workspace);
}

pub enum Intent {
    ConfigureMusic,
    ImportData,
    ExportData(Option<usize>),
    Objects(crate::tabs::database_objects::window::Action),
    SaveDesign,
    RefreshDesign,
    LoadDesignSql,
    RefreshDesignInfo(tomato_core::table_design::alter::InfoPage),
    CreateDesign(String),
    Close,
    ReconnectTerminal,
    ConnectDesktop,
    ToggleAdbWindow,
    SubmitVncPassword,
    Command(Box<Command>),
    Edit(PathBuf),
    OpenFileDocument(String),
    Save(PathBuf, String),
    Markdown {
        source: String,
        revision: u64,
    },
    MarkdownImage(String),
    PrintMarkdown,
    ExportMarkdownHtml,
    LoadSql(PathBuf),
    SaveQuery,
    SqlConnection(String),
    SqlDatabase(String),
    AiSql {
        selected_prompt: bool,
    },
    AiSqlSettings,
    StartConfigured(tomato_core::server::ServerConfig),
    ConfigureServer(tomato_core::server::ServerConfig),
    SshInspect {
        connection: tomato_core::Connection,
        id: String,
    },
    SshCommand {
        connection: tomato_core::Connection,
        command: String,
        title: String,
    },
}

impl Intent {
    fn command(command: Command) -> Self {
        Self::Command(Box::new(command))
    }
}

#[cfg(test)]
pub fn render(ui: &mut Ui, tab: &mut Tab, icons: &mut crate::icons::Icons) -> Option<Intent> {
    render_with_connections(ui, tab, icons, &[])
}
pub fn render_with_connections(
    ui: &mut Ui,
    tab: &mut Tab,
    icons: &mut crate::icons::Icons,
    catalog: &[tomato_core::Connection],
) -> Option<Intent> {
    let mut intent = None;
    let sql_editor = matches!(&tab.panel,Panel::Sql {workbench,..}if workbench.table.is_none());
    let designer = matches!(&tab.panel, Panel::Sql {workbench,..} if workbench.designer.is_some());
    let sql_table = matches!(&tab.panel, Panel::Sql {workbench,..} if workbench.table.is_some());
    if !sql_editor
        && !designer
        && !sql_table
        && let Some(error) = &tab.error
    {
        ui.colored_label(Color32::from_rgb(220, 80, 80), error);
        ui.separator();
    }
    let owns_progress = sql_editor
        || designer
        || matches!(&tab.panel, Panel::Mongo { .. } | Panel::Redis { .. })
        || matches!(&tab.panel,Panel::Tool(tool) if matches!(tool.kind.as_str(),"hosts_file"|"trae_session"|"resource_extractor"|"music_downloader"|"game_dungeons"));
    // Keep the loaded grid in place during refreshes; `busy` still guards edits
    // and duplicate requests, and failures still use the error display above.
    let loaded_table = matches!(&tab.panel, Panel::Sql { workbench, .. }
        if workbench.table.is_some() && workbench.page.is_some());
    if tab.busy && !owns_progress && !loaded_table {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("正在处理…");
        });
    }
    match &mut tab.panel {
        Panel::Cloud { .. } => return cloud::render(ui, tab, catalog),
        Panel::MessageQueue { .. } => return mq::render(ui, tab, icons),
        Panel::Mongo { .. } => return mongo::render(ui, tab),
        Panel::Sql { .. } => {
            return sql::render_with_connections(ui, tab, icons, catalog);
        }
        Panel::Redis { .. } => {
            return redis::render(ui, tab, icons);
        }
        Panel::Editor { .. } => {
            return editor::render(ui, tab);
        }
        Panel::Terminal { .. } => {
            return terminal::render(ui, tab, icons);
        }
        Panel::NativeDesktop { .. } => return desktop::render(ui, tab),
        Panel::Desktop(connection) => {
            if connection.kind() == "SPICE" {
                ui.heading("SPICE 暂不可用");
                ui.label("Tomato 不会再启动外部远程桌面客户端");
                ui.label("内置 VNC 客户端已经可用；内置 SPICE 协议仍在开发中。");
                return None;
            }
        }
        Panel::Server(_) => return server::render(ui, tab, icons),
        Panel::Tool(tool) => {
            intent = tool_panel(ui, tool, tab.busy, &mut tab.error, icons, catalog)
        }
        Panel::Unavailable { kind } => {
            ui.heading(format!("{kind} · 待迁移"));
            ui.label("连接配置已保留，当前 Rust 版本还没有此协议适配器。完整范围与进度见 docs/migration.md。");
        }
    }
    intent
}

pub(crate) fn render_query_result_dialog(
    ctx: &egui::Context,
    result: &tomato_services::QueryResult,
    state: &mut crate::tabs::query_results::State,
    id: egui::Id,
) {
    sql::result_dialog(ctx, result, state, id);
}

pub(crate) fn sql_preview_layout(
    ui: &Ui,
    buffer: &dyn egui::TextBuffer,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    sql::preview_layout(ui, buffer, width)
}

pub(crate) use sql::export_table_snapshot;
