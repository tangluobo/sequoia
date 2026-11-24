use super::*;
use crate::{icons, theme};

const MODULES: &[(&str, &str)] = &[
    ("connect", "连接"),
    ("tools", "工具"),
    ("server", "服务器"),
    ("development", "开发"),
    ("settings", "设置"),
];

fn header_divider(ui: &Ui, header: egui::Rect) {
    // JavaFX's bottom border belongs inside its 52px header. A stroke centered
    // on bottom() falls into the following panel and its background covers it.
    let border = egui::Rect::from_min_max(
        egui::pos2(header.left(), header.bottom() - 1.0),
        header.right_bottom(),
    );
    ui.painter()
        .with_clip_rect(ui.clip_rect().intersect(header))
        .rect_filled(border, 0, theme::HEADER_DIVIDER);
}
const TOOL_ITEMS: &[(&str, &str, &str)] = &[
    (
        "image_format_converter",
        "图片格式转换",
        "SVG转PNG等图片格式转换",
    ),
    (
        "dataset_converter",
        "数据集格式转换",
        "PASCAL VOC转COCO格式",
    ),
    ("json_tool", "JSON处理工具", "格式化、压缩、编码转换"),
    (
        "desktop_shortcut",
        "桌面快捷方式",
        "创建Linux .desktop快捷方式",
    ),
    (
        "hosts_file",
        "Hosts文件管理",
        "管理和切换不同环境的Hosts配置",
    ),
    (
        "image_background_remover",
        "图标制作工具",
        "图片背景透明化与裁切生成图标",
    ),
    (
        "trae_session",
        "Trae会话管理",
        "管理 Trae CN 多账号会话切换",
    ),
    (
        "resource_extractor",
        "资源图标提取",
        "从EXE/DLL中提取嵌入资源",
    ),
    ("downloader", "下载器", "多线程分段下载与动态任务再分配"),
];

pub(super) fn tab_icon(tab: &Tab) -> Option<&'static str> {
    match &tab.panel {
        Panel::Cloud { connection, .. } => Some(crate::icons::connection(connection.kind())),
        Panel::MessageQueue { connection, .. } => Some(crate::icons::connection(connection.kind())),
        Panel::Redis { .. } => Some("images/connect/redis.png"),
        Panel::Sql { workbench, .. } if workbench.browser.is_some() => {
            Some("images/connect/folder.png")
        }
        Panel::Sql { workbench, .. }
            if workbench.table.is_some() || workbench.designer.is_some() =>
        {
            Some(
                if workbench
                    .definition
                    .as_ref()
                    .map_or(workbench.table_is_view, |definition| definition.view)
                {
                    "images/connect/view.png"
                } else {
                    "images/connect/table.png"
                },
            )
        }
        Panel::Sql { .. } => Some("images/connect/query.png"),
        _ => None,
    }
}

impl TomatoApp {
    pub fn ui(&mut self, ui: &mut Ui) {
        crate::table_style::sync(ui.ctx(), self.settings.table_font(), false);
        if self.closing() {
            ui.painter().rect_filled(ui.max_rect(), 0, Color32::WHITE);
            ui.centered_and_justified(|ui| {
                ui.label("正在关闭，清理后台任务…");
            });
            return;
        }
        ui.ctx().set_cursor_image(None);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        if ui.ctx().viewport_id() != egui::ViewportId::ROOT {
            self.rdp_viewport_ui(ui);
            return;
        }
        let outer = ui.max_rect();
        ui.painter().rect_filled(outer, 0, Color32::WHITE);
        ui.painter().rect_stroke(
            outer,
            0,
            egui::Stroke::new(1.0, Color32::from_rgb(190, 190, 188)),
            egui::StrokeKind::Inside,
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(outer.shrink(1.0)), |ui| {
            if self.root_modal_open() {
                let opacity = ui.opacity();
                ui.disable();
                if self.visible_vnc_password() {
                    ui.set_opacity(opacity);
                }
            }
            self.navigation(ui);
            if self.section != 4 && self.settings.sidebar_visible {
                self.sidebar(ui);
            }
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::WHITE))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let keep_focus = self.window_header(ui);
                    ui.set_clip_rect(ui.clip_rect().intersect(ui.available_rect_before_wrap()));
                    if super::preview_development::enabled() {
                        return;
                    }
                    // Caption interactions resize or move the window around its focused content.
                    let focus_policy = keep_focus.then(|| {
                        ui.memory_mut(|memory| {
                            std::mem::replace(
                                &mut memory.options.input_options.surrender_focus_on,
                                egui::SurrenderFocusOn::Never,
                            )
                        })
                    });
                    match self.section {
                        0 => self.workspace_view(ui, true),
                        1 => {
                            if self.active.is_some() {
                                self.workspace_view(ui, false);
                            } else {
                                message(
                                    ui,
                                    "选择一个工具开始使用",
                                    "从左侧工具列表中选择需要的功能",
                                );
                            }
                        }
                        2 => {
                            if self.active.is_some() {
                                self.workspace_view(ui, false);
                            } else {
                                message(
                                    ui,
                                    "选择一个服务类型开始使用",
                                    "从左侧选择 HTTP / FTP / SMB",
                                );
                            }
                        }
                        3 => {
                            self.ensure_development();
                            if let Some(state) = &mut self.development {
                                state.show(ui, &mut self.icons);
                            }
                        }
                        _ => self.settings_view(ui),
                    }
                    if let Some(policy) = focus_policy {
                        ui.memory_mut(|memory| {
                            memory.options.input_options.surrender_focus_on = policy;
                        });
                    }
                });
        });
        let ctx = ui.ctx().clone();
        let minimized = ctx.input(|input| input.viewport().minimized == Some(true));
        for tab in &mut self.tabs {
            if let Panel::Editor { markdown, .. } = &mut tab.panel {
                markdown.hide_pictures(&ctx, minimized);
            }
        }
        resize_edges(ui, outer);
        self.type_picker(&ctx);
        self.collaboration_ui(&ctx);
        if let Some(state) = &mut self.input_share {
            state.show(&ctx);
        }
        self.poll_preview_device();
        self.connection_dialog(&ctx);
        self.import_dialog(&ctx);
        self.connection_export_dialog(&ctx);
        self.credential_dialog(&ctx);
        self.markdown_print_dialog(&ctx);
        self.markdown_html_dialog(&ctx);
        self.ssh_tree_auth(&ctx);
        self.mongo_dialog(&ctx);
        self.confirm_dialog(&ctx);
        self.query_dialog(&ctx);
        self.sql_results_preview(&ctx);
        self.sql_workbench_preview(&ctx);
        self.query_input_preview(&ctx);
        self.database_recovery_preview(&ctx);
        self.preview_converter_ready(&ctx);
        self.json_selection_preview_ready(&ctx);
        self.file_drop_preview(&ctx);
        self.binary_table_preview(&ctx);
        self.development_preview(&ctx);
        self.table_designer_preview(&ctx);
        self.query_result_dialogs(&ctx);
        self.ai_sql_dialog(&ctx);
        self.backup_dialog(&ctx);
        self.data_export_dialog(&ctx);
        self.data_import_dialog(&ctx);
        self.table_transfer_dialog(&ctx);
        self.table_paste_dialog(&ctx);
        self.database_admin_dialog(&ctx);
        self.rdp_windows(&ctx);
        self.session_config_dialog(&ctx);
        self.table_config_dialog(&ctx);
        self.record_terminal_config_fixture();
        self.tree_objects_dialog(&ctx);
        self.backup_file_dialog(&ctx);
        self.local_tree_dialog(&ctx);
        self.remote_properties(&ctx);
        if let Some(message) = self.error.clone() {
            egui::Window::new("操作失败")
                .collapsible(false)
                .resizable(false)
                .show(&ctx, |ui| {
                    ui.label(message);
                    if ui.button("确定").clicked() {
                        self.error = None;
                    }
                });
        }
        if self.tabs.iter().any(|tab| tab.busy) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn navigation(&mut self, ui: &mut Ui) {
        egui::Panel::left("navigation")
            .exact_size(theme::NAV_WIDTH)
            .resizable(false)
            .show_separator_line(false)
            .frame(egui::Frame::NONE.fill(theme::NAV_BACKGROUND))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                self.layout.nav = ui.max_rect();
                ui.add_space(20.0);
                for (index, (name, label)) in MODULES.iter().enumerate() {
                    let (row, response) = ui.allocate_exact_size(
                        egui::vec2(theme::NAV_WIDTH, if index == 1 { 45.0 } else { 40.0 }),
                        egui::Sense::click(),
                    );
                    if response.hovered() {
                        ui.painter().rect_filled(
                            egui::Rect::from_center_size(row.center(), egui::vec2(40.0, 40.0)),
                            8,
                            Color32::from_gray(224),
                        );
                    }
                    self.icons.paint(
                        ui,
                        &format!("nav/{name}.svg"),
                        egui::Rect::from_center_size(row.center(), egui::vec2(24.0, 24.0)),
                    );
                    if response.on_hover_text(*label).clicked() {
                        self.switch_module(index);
                    }
                    ui.add_space(10.0);
                }
                if ui.button("键鼠").on_hover_text("鼠标和键盘共享").clicked()
                    && let Some(runtime) = &self.runtime
                {
                    self.input_share
                        .get_or_insert_with(|| {
                            Box::new(super::input_share::State::new(runtime, ui.ctx()))
                        })
                        .open = true;
                }
                // Keep navigation's border inside its own background, like
                // the header border; the neighbouring panel owns its fill.
                let rect = ui.max_rect();
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 1., rect.top()),
                        rect.right_bottom(),
                    ),
                    0,
                    theme::DIVIDER,
                );
            });
    }
    fn switch_module(&mut self, index: usize) {
        if index == self.section {
            return;
        }
        self.module_active[self.section] = self.active.take();
        self.section = index;
        self.active = self.module_active[index]
            .clone()
            .filter(|id| self.tabs.iter().any(|tab| &tab.id == id));
        self.filter.clear();
    }
    fn sidebar(&mut self, ui: &mut Ui) {
        egui::Panel::left("sidebar")
            // Include the separator's one-point outer margin in the panel's
            // size, preserving the existing content width and resize limits.
            // This panel owns both the idle border and its resize highlight.
            .show_separator_line(true)
            .default_size(theme::SIDEBAR_WIDTH + 1.)
            .min_size(151.0)
            .max_size(501.0)
            .resizable(true)
            .frame(egui::Frame::NONE.fill(Color32::WHITE))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                self.layout.sidebar = ui.max_rect();
                let (header, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), theme::HEADER_HEIGHT),
                    egui::Sense::hover(),
                );
                header_divider(ui, header);
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(header.shrink2(egui::vec2(15.0, 10.0))),
                    |ui| {
                        if self.section == 2 {
                            ui.strong("服务类型");
                        } else if self.section == 3 {
                            crate::panels::ensure_bold_font(ui.ctx());
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.;
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(20., 20.),
                                    egui::Sense::hover(),
                                );
                                self.icons.paint(ui, "images/development/folder.svg", rect);
                                ui.label(egui::RichText::new("开发目录").font(egui::FontId::new(
                                    14.,
                                    egui::FontFamily::Name("md-bold".into()),
                                )));
                            });
                        } else {
                            let hint = if self.section == 0 {
                                "搜索"
                            } else {
                                "搜索工具..."
                            };
                            egui::Frame::new()
                                .fill(if self.section == 0 {
                                    Color32::from_gray(240)
                                } else {
                                    Color32::WHITE
                                })
                                .corner_radius(6)
                                .inner_margin(egui::Margin::symmetric(10, 5))
                                .show(ui, |ui| {
                                    let response = ui.add(
                                        TextEdit::singleline(&mut self.filter)
                                            .frame(egui::Frame::NONE)
                                            .hint_text(hint)
                                            .desired_width(ui.available_width())
                                            .font(egui::FontId::proportional(13.0)),
                                    );
                                    if response.changed()
                                        && self.filter.trim_matches(|c: char| c <= ' ').is_empty()
                                    {
                                        self.collapsed = self
                                            .store
                                            .connections
                                            .iter()
                                            .map(|c| c.id.clone())
                                            .collect();
                                    }
                                });
                        }
                    },
                );
                // The search editor is laid out inside an already allocated
                // header. Its scoped child may end above that allocation's
                // bottom; restore the body cursor before constructing the tree.
                ui.advance_cursor_after_rect(header);
                let body = egui::Rect::from_min_max(
                    egui::pos2(ui.max_rect().left(), header.bottom()),
                    ui.max_rect().max,
                );
                ui.set_clip_rect(ui.clip_rect().intersect(body));
                if self.section == 0 {
                    let body = ui.available_rect_before_wrap();
                    let blank = ui.interact(
                        body,
                        ui.make_persistent_id("connection-context"),
                        egui::Sense::click(),
                    );
                    self.connection_list(ui);
                    blank.context_menu(|ui| self.create_menu(ui));
                } else if self.section == 3 {
                    self.ensure_development();
                    if let Some(state) = &mut self.development {
                        state.poll(ui.ctx());
                        state.sidebar(ui, &mut self.icons);
                    }
                } else {
                    ScrollArea::vertical().show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                        if self.section == 1 {
                            for &(kind, label, description) in TOOL_ITEMS {
                                if !self.filter.is_empty()
                                    && !label.to_lowercase().contains(&self.filter.to_lowercase())
                                {
                                    continue;
                                }
                                let selected =
                                    self.active_source() == Some(format!("tool:{kind}").as_str());
                                if self.list_card(
                                    ui,
                                    &icons::tool(kind),
                                    label,
                                    description,
                                    selected,
                                ) {
                                    self.open_tool(kind, label);
                                }
                            }
                        } else if self.section == 2 {
                            for (kind, label, description) in [
                                (
                                    "HTTP_SERVER",
                                    "HTTP服务器",
                                    "HTTP 文件服务器，支持浏览器访问",
                                ),
                                (
                                    "FTP_SERVER",
                                    "FTP服务器",
                                    "FTP 文件服务器，支持 FTP 客户端访问",
                                ),
                                (
                                    "SMB_SERVER",
                                    "SMB文件共享",
                                    "SMB 文件共享服务器，局域网文件共享",
                                ),
                            ] {
                                let selected =
                                    self.active_source() == Some(format!("server:{kind}").as_str());
                                if self.list_card(
                                    ui,
                                    icons::connection(kind),
                                    label,
                                    description,
                                    selected,
                                ) {
                                    self.open_server(kind, label);
                                }
                            }
                        }
                    });
                }
            });
    }
    fn list_card(
        &mut self,
        ui: &mut Ui,
        icon: &str,
        label: &str,
        description: &str,
        selected: bool,
    ) -> bool {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 61.0), egui::Sense::click());
        let fill = if selected {
            Color32::from_rgb(232, 244, 255)
        } else if response.hovered() {
            Color32::from_gray(245)
        } else {
            Color32::WHITE
        };
        ui.painter().rect_filled(rect, 0, fill);
        let tile =
            egui::Rect::from_min_size(rect.min + egui::vec2(12.0, 10.0), egui::vec2(40.0, 40.0));
        ui.painter()
            .rect_filled(tile, 8, Color32::from_rgb(232, 244, 255));
        self.icons.paint(ui, icon, tile.shrink(8.0));
        ui.painter().text(
            rect.min + egui::vec2(62.0, 13.0),
            egui::Align2::LEFT_TOP,
            label,
            theme::java_font(14.0),
            Color32::from_gray(51),
        );
        let painter = ui
            .painter()
            .with_clip_rect(rect.shrink2(egui::vec2(5.0, 0.0)));
        painter.text(
            rect.min + egui::vec2(62.0, 34.0),
            egui::Align2::LEFT_TOP,
            description,
            theme::java_font(11.0),
            Color32::from_gray(136),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.bottom(),
            egui::Stroke::new(1.0, Color32::from_gray(240)),
        );
        response.clicked()
    }
    fn window_header(&mut self, ui: &mut Ui) -> bool {
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme::HEADER_HEIGHT),
            egui::Sense::hover(),
        );
        self.layout.header = rect;
        header_divider(ui, rect);
        let cloud =
            egui::Rect::from_min_size(rect.min + egui::vec2(12.0, 7.0), egui::vec2(106.0, 28.0));
        if ui.place(cloud, egui::Button::new("云端协作")).clicked() {
            self.open_collaboration();
        }
        let button_start = rect.right() - 126.0;
        let drag = egui::Rect::from_min_max(
            rect.min + egui::vec2(124.0, 0.0),
            egui::pos2(button_start, rect.bottom()),
        );
        let response = ui.interact(
            drag,
            ui.make_persistent_id("window-drag"),
            egui::Sense::click_and_drag(),
        );
        if response.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if response.double_clicked() {
            toggle_maximized(ui.ctx());
        }
        let mut keep_focus = response.is_pointer_button_down_on() || response.clicked();
        for index in 0..4 {
            let button = egui::Rect::from_min_size(
                egui::pos2(button_start + index as f32 * 32.0, rect.top()),
                egui::vec2(30.0, 26.0),
            );
            let response = ui.interact(
                button,
                ui.make_persistent_id(("window-button", index)),
                egui::Sense::click(),
            );
            if index == 2 {
                keep_focus |= response.is_pointer_button_down_on() || response.clicked();
            }
            let hovered = response.hovered();
            if hovered {
                ui.painter().rect_filled(
                    button,
                    0,
                    if index == 3 {
                        Color32::from_rgb(196, 43, 28)
                    } else {
                        Color32::from_gray(229)
                    },
                );
            }
            let color = if index == 3 && hovered {
                Color32::WHITE
            } else {
                Color32::from_gray(19)
            };
            let center = button.center();
            let stroke = egui::Stroke::new(1.0, color);
            match index {
                0 => self.icons.paint(
                    ui,
                    "images/settings.png",
                    egui::Rect::from_center_size(center, egui::vec2(18.0, 18.0)),
                ),
                1 => {
                    ui.painter().line_segment(
                        [
                            center + egui::vec2(-6.0, 0.0),
                            center + egui::vec2(6.0, 0.0),
                        ],
                        stroke,
                    );
                }
                2 => {
                    ui.painter().rect_stroke(
                        egui::Rect::from_center_size(center, egui::vec2(12.0, 10.0)),
                        0,
                        stroke,
                        egui::StrokeKind::Inside,
                    );
                }
                _ => {
                    for sign in [-1.0, 1.0] {
                        ui.painter().line_segment(
                            [
                                center + egui::vec2(-6.0, -6.0 * sign),
                                center + egui::vec2(6.0, 6.0 * sign),
                            ],
                            stroke,
                        );
                    }
                }
            }
            if response.clicked() {
                match index {
                    0 => self.switch_module(4),
                    1 => ui
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                    2 => toggle_maximized(ui.ctx()),
                    _ => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
                }
            }
        }
        keep_focus
    }
    pub(super) fn workspace_view(&mut self, ui: &mut Ui, tabs_visible: bool) {
        let mut close = None;
        let mut refresh_redis = None;
        let mut refresh_objects = None;
        let mut close_group = None;
        let mut rdp_action = None;
        let mut terminal_action = None;
        let mut file_action = None;
        let mut table_action = None;
        let mut desktop_action = None;
        let visible_ids = self
            .tabs
            .iter()
            .filter(|tab| tab.workspace_visible())
            .map(|tab| tab.id.clone())
            .collect::<Vec<_>>();
        if tabs_visible && self.tabs.iter().any(|tab| tab.workspace_visible()) {
            let (strip, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), theme::TAB_HEIGHT),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(strip, 0, theme::NAV_BACKGROUND);
            let overflowing = self
                .tabs
                .iter()
                .filter(|tab| tab.workspace_visible())
                .map(|tab| super::tab_strip::width(ui, tab))
                .sum::<f32>()
                > strip.width();
            let list = egui::Rect::from_min_max(
                strip.min,
                strip.max - egui::vec2(if overflowing { 26. } else { 0. }, 0.),
            );
            let active_key = ui.make_persistent_id("last-visible-tab");
            let changed = ui
                .ctx()
                .data(|data| data.get_temp::<String>(active_key))
                .as_deref()
                != self.active.as_deref();
            ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(list));
                ScrollArea::horizontal().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                        for tab in self.tabs.iter().filter(|tab| {
                            tab.workspace_visible()
                        }) {
                            let redis = matches!(tab.panel, Panel::Redis { .. });
                            let object_browser=matches!(&tab.panel,Panel::Sql{workbench,..}if workbench.browser.is_some());
                            let header = super::tab_strip::header(ui, tab, &mut self.icons, self.active.as_deref() == Some(&tab.id));
                            let response = header.response;
                            if changed&&self.active.as_deref()==Some(tab.id.as_str()){response.scroll_to_me(Some(egui::Align::Center));}
                            if header.close { close = Some(tab.id.clone()); }
                            if response.clicked() {
                                self.active = Some(tab.id.clone());
                            }
                            response.context_menu(|ui|{
                                if let Some(action)=super::file_tabs::menu(ui,tab){file_action=Some((tab.id.clone(),action));}
                                if let Some(action)=super::terminal_config::menu(ui,tab){terminal_action=Some((tab.id.clone(),action));}
                                if let Some(action)=super::table_config::menu(ui,tab){table_action=Some((tab.id.clone(),action));}
                                if let Some(action)=super::desktops::menu::show(ui,tab){desktop_action=Some((tab.id.clone(),action));}
                                if matches!(&tab.panel,Panel::NativeDesktop{connection,..}if connection.kind()=="RDP") {
                                    for(label,action)in[("全屏",super::rdp_windows::Action::Fullscreen),("会话配置",super::rdp_windows::Action::Session),("全局配置",super::rdp_windows::Action::Global)] {
                                        if ui.add(crate::compact_menu::Item::new(label)).clicked(){rdp_action=Some((tab.id.clone(),action));ui.close();}
                                    }
                                }

                                if redis&&ui.add_enabled(!tab.busy,crate::compact_menu::Item::new("刷新")).clicked(){refresh_redis=Some(tab.id.clone());ui.close();}
                                if object_browser&&ui.add_enabled(!tab.busy,crate::compact_menu::Item::new("刷新对象")).clicked(){refresh_objects=Some(tab.id.clone());ui.close();}
                                if matches!(tab.panel,Panel::Sql{..}){
                                    ui.separator();let index=visible_ids.iter().position(|id|id==&tab.id).unwrap_or_default();
                                    for(label,side,enabled)in [("关闭左侧",super::tab_closes::Side::Left,index>0),("关闭右侧",super::tab_closes::Side::Right,index+1<visible_ids.len()),("关闭全部",super::tab_closes::Side::All,true)]{if ui.add_enabled(enabled,crate::compact_menu::Item::new(label)).clicked(){close_group=Some((tab.id.clone(),side));ui.close();}}
                                }
                            });

                        }
                    });
                });
            });
            if let Some(active) = &self.active {
                ui.ctx()
                    .data_mut(|data| data.insert_temp(active_key, active.clone()));
            }
            if overflowing {
                self.tab_overflow(
                    ui,
                    egui::Rect::from_min_max(
                        egui::pos2(strip.right() - 16., strip.top()),
                        strip.max,
                    ),
                );
            }
            ui.advance_cursor_after_rect(strip);
            let boundary = egui::Rect::from_min_max(
                egui::pos2(ui.max_rect().left(), strip.bottom()),
                ui.max_rect().max,
            );
            ui.set_clip_rect(ui.clip_rect().intersect(boundary));
        }
        if let Some((id, action)) = rdp_action {
            self.rdp_action(&id, action);
        }
        if let Some((id, action)) = desktop_action {
            self.desktop_menu_action(&id, action);
        }
        if let Some((id, action)) = table_action {
            self.table_tab_action(&id, action);
        }
        if let Some((id, action)) = file_action {
            self.file_tab_action(&id, action);
        }
        if let Some((id, action)) = terminal_action {
            self.terminal_tab_action(&id, action);
        }
        if let Some((id, side)) = close_group {
            self.close_tab_group(&id, side);
        }
        if let Some(id) = refresh_objects {
            self.object_action(&id, crate::tabs::database_objects::window::Action::Refresh);
        }
        if let Some(id) = refresh_redis {
            let command =
                self.tabs
                    .iter_mut()
                    .find(|t| t.id == id)
                    .and_then(|t| match &mut t.panel {
                        Panel::Redis { connection, view } => {
                            if view.dialog.is_some() || view.failure.is_some() {
                                return None;
                            }
                            let action = view.begin_scan();
                            let mut connection = (**connection).clone();
                            connection.set("redisDatabase", view.database);
                            Some(Command::RedisData { connection, action })
                        }
                        _ => None,
                    });
            if let Some(command) = command {
                self.submit(&id, command, Purpose::Normal);
            }
        }
        if let Some(id) = close {
            if self.tabs.iter().any(|tab| tab.id == id && tab.dirty()) {
                self.confirmation = Some(Confirmation::Close(id));
            } else {
                self.close(&id);
            }
        }
        if let Some(index) = self.tabs.iter().position(|tab| {
            Some(&tab.id) == self.active.as_ref()
                && !matches!(&tab.panel,Panel::NativeDesktop{view,..}if view.window.is_some())
        }) {
            let id = self.tabs[index].id.clone();
            self.load_object_tab(&id);
            let intent = ui
                .push_id(&id, |ui| {
                    ui.set_clip_rect(ui.clip_rect().intersect(ui.available_rect_before_wrap()));
                    if matches!(
                        std::env::var("TOMATO_UI_PREVIEW").as_deref(),
                        Ok("sql_workbench" | "table_designer" | "table_data" | "tab_strip")
                    ) && std::env::var_os("TOMATO_UI_CAPTURE").is_some()
                    {
                        return None;
                    }
                    if let Some(rendered) = self.render_file_documents(ui, index) {
                        return rendered.intent;
                    }
                    panels::render_with_connections(
                        ui,
                        &mut self.tabs[index],
                        &mut self.icons,
                        &self.store.connections,
                    )
                    .map(|intent| (id.clone(), intent))
                })
                .inner;
            if let Some((target, intent)) = intent {
                self.handle_intent(&target, intent);
            }
        }
    }
    pub(super) fn create_menu(&mut self, ui: &mut Ui) {
        if ui.button("新建连接").clicked() {
            self.new_connection_at(None);
            ui.close();
        }
        if ui.button("新建目录").clicked() {
            self.new_folder_at(None);
            ui.close();
        }
        ui.separator();
        if ui.button("导入连接").clicked() {
            self.import();
            ui.close();
        }
        if ui.button("导出连接").clicked() {
            self.export();
            ui.close();
        }
    }
    fn open_server(&mut self, kind: &str, label: &str) {
        let source = format!("server:{kind}");
        if let Some(tab) = self.tabs.iter().find(|tab| tab.source_id == source) {
            self.active = Some(tab.id.clone());
            return;
        }
        let mut config = Connection::new(kind);
        config.name = label.into();
        match tabs::create_tab(config, self.runtime.as_ref().map(|r| r.handle())) {
            Ok(mut tab) => {
                tab.source_id = source;
                self.active = Some(tab.id.clone());
                self.tabs.push(tab);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }
}

fn resize_edges(ui: &mut Ui, rect: egui::Rect) {
    if ui
        .ctx()
        .input(|input| input.viewport().maximized.unwrap_or(false))
    {
        return;
    }
    use egui::ResizeDirection as D;
    let edge = 3.0;
    let corner = 8.0;
    for (index, (area, direction)) in [
        (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + edge)),
            D::North,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(rect.left(), rect.bottom() - edge), rect.max),
            D::South,
        ),
        (
            egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + edge, rect.bottom())),
            D::West,
        ),
        (
            egui::Rect::from_min_max(egui::pos2(rect.right() - edge, rect.top()), rect.max),
            D::East,
        ),
        (
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(corner, corner)),
            D::NorthWest,
        ),
        (
            egui::Rect::from_min_size(
                rect.right_top() - egui::vec2(corner, 0.0),
                egui::vec2(corner, corner),
            ),
            D::NorthEast,
        ),
        (
            egui::Rect::from_min_size(
                rect.left_bottom() - egui::vec2(0.0, corner),
                egui::vec2(corner, corner),
            ),
            D::SouthWest,
        ),
        (
            egui::Rect::from_min_size(
                rect.right_bottom() - egui::vec2(corner, corner),
                egui::vec2(corner, corner),
            ),
            D::SouthEast,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let cursor = match direction {
            D::North | D::South => egui::CursorIcon::ResizeVertical,
            D::East | D::West => egui::CursorIcon::ResizeHorizontal,
            D::NorthWest | D::SouthEast => egui::CursorIcon::ResizeNwSe,
            _ => egui::CursorIcon::ResizeNeSw,
        };
        let response = ui
            .interact(
                area,
                ui.id().with(("window-edge", index)),
                egui::Sense::drag(),
            )
            .on_hover_cursor(cursor);
        if response.drag_started() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}
fn message(ui: &mut Ui, title: &str, hint: &str) {
    let rect = ui.available_rect_before_wrap();
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 16.0),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(18.0),
        Color32::from_gray(153),
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 20.0),
        egui::Align2::CENTER_CENTER,
        hint,
        egui::FontId::proportional(13.0),
        Color32::from_gray(187),
    );
}
fn toggle_maximized(ctx: &egui::Context) {
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
}
