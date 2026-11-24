use super::*;
pub(super) fn place_left(
    ui: &mut Ui,
    rect: egui::Rect,
    widget: impl egui::Widget,
) -> egui::Response {
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    )
    .add(widget)
}

pub(super) fn tool_panel(
    ui: &mut Ui,
    tool: &mut ToolState,
    busy: bool,
    _error: &mut Option<String>,
    icons: &mut crate::icons::Icons,
    catalog: &[tomato_core::Connection],
) -> Option<Intent> {
    if tool.kind == "game_dungeons" {
        return super::game_dungeons::render(
            ui,
            tool.game_dungeons.get_or_insert_with(Default::default),
            busy,
            _error.as_deref(),
            icons,
        );
    }
    if tool.kind == "port_mapping" {
        let state = tool.port_mapping.get_or_insert_with(|| {
            Box::new(crate::tabs::port_mapping::State::new(
                tomato_services::terminal::ssh::forwarding::legacy_path(),
                None,
                tomato_core::Store::default_path().with_file_name("known_hosts"),
            ))
        });
        return super::port_mapping::render(ui, state, catalog);
    }
    if tool.kind == "downloader" {
        return super::downloader::render(ui, tool, icons);
    }
    if tool.kind == "music_downloader" {
        return super::music::render(
            ui,
            tool.music.get_or_insert_with(Default::default),
            busy,
            icons,
        );
    }
    if tool.kind == "desktop_shortcut" {
        return super::shortcut::render(ui, tool, busy, icons);
    }
    if tool.kind == "image_background_remover" {
        return super::icon_tool::render(ui, tool, icons);
    }
    if tool.kind == "resource_extractor" {
        return super::extractor::render(ui, tool, busy, icons);
    }
    if tool.kind == "trae_session" {
        return super::trae::render(ui, tool, busy, icons);
    }
    if tool.kind == "hosts_file" {
        return super::hosts::render(ui, tool, busy, icons);
    }
    if tool.kind == "json_tool" {
        return super::json::render(ui, tool, busy, icons);
    }
    if tool.kind == "image_format_converter" || tool.kind == "dataset_converter" {
        return super::converters::render(ui, tool, busy, icons);
    }
    if tool.kind == "color_transpose_game" {
        super::color_transpose::show(
            ui,
            tool.color_transpose.get_or_insert_with(Default::default),
        );
        return None;
    }
    let mut intent = None;
    ui.heading(
        tomato_core::catalog::TOOLS
            .iter()
            .find(|(id, _)| *id == tool.kind)
            .map(|(_, label)| *label)
            .unwrap_or("工具"),
    );
    match tool.kind.as_str() {
        "hosts_file" => {
            let path = if cfg!(windows) {
                PathBuf::from(std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into()))
                    .join("System32/drivers/etc/hosts")
            } else {
                PathBuf::from("/etc/hosts")
            };
            ui.label("系统权限由操作系统控制；保存前请核对文件内容。");
            if ui.button("打开 Hosts 编辑器").clicked() {
                intent = Some(Intent::Edit(path));
            }
        }
        _ => {
            ui.label("此工具的 Rust 实现待迁移，原配置会保留。");
        }
    }
    if !tool.output.is_empty() {
        ScrollArea::both().id_salt("tool-output").show(ui, |ui| {
            ui.monospace(&tool.output);
        });
    }
    intent
}

pub(super) fn header(
    ui: &mut Ui,
    icons: &mut crate::icons::Icons,
    kind: &str,
    title: &str,
    subtitle: &str,
) {
    let json = kind == "json_tool";
    let bold = matches!(
        kind,
        "desktop_shortcut"
            | "image_background_remover"
            | "downloader"
            | "image_format_converter"
            | "dataset_converter"
    );
    if bold {
        super::editor::ensure_bold_cjk(ui.ctx());
    }
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width(),
            if json {
                46.
            } else if matches!(
                kind,
                "image_background_remover"
                    | "downloader"
                    | "image_format_converter"
                    | "dataset_converter"
            ) {
                51.
            } else if kind == "desktop_shortcut" {
                50.
            } else {
                48.
            },
        ),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0, Color32::from_rgb(247, 248, 250));
    let icon = match kind {
        "image_format_converter" => "images/tools/image-converter-header.svg".into(),
        "dataset_converter" => "images/tools/dataset-converter-header.svg".into(),
        "json_tool" => "images/tools/json-header.svg".into(),
        "desktop_shortcut" => "images/tools/shortcut-header.svg".into(),
        "image_background_remover" => "images/tools/icon-header.svg".into(),
        "downloader" => "images/tools/downloader-header.svg".into(),
        _ => crate::icons::tool(kind),
    };
    if matches!(
        kind,
        "image_background_remover" | "downloader" | "image_format_converter" | "dataset_converter"
    ) {
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1., Color32::from_gray(232)),
        );
    }
    icons.paint(
        ui,
        &icon,
        egui::Rect::from_min_size(
            rect.min + egui::vec2(if kind == "downloader" { 16. } else { 20. }, 15.0),
            egui::vec2(18.0, 18.0),
        ),
    );
    ui.painter().text(
        rect.left_center()
            + egui::vec2(
                if json {
                    46.
                } else if kind == "dataset_converter" {
                    52.
                } else if kind == "downloader" {
                    40.
                } else {
                    48.
                },
                0.0,
            ),
        egui::Align2::LEFT_CENTER,
        title,
        if bold {
            egui::FontId::new(16., egui::FontFamily::Name("md-bold".into()))
        } else {
            egui::FontId::proportional(16.0)
        },
        Color32::from_gray(51),
    );
    ui.painter().text(
        rect.right_center() - egui::vec2(20.0, 0.0),
        egui::Align2::RIGHT_CENTER,
        subtitle,
        egui::FontId::proportional(12.0),
        Color32::from_gray(153),
    );
}
