use super::*;
use crate::native_dialog::{button, font};
use egui::{Align2, Color32, Rect, vec2};
use tomato_core::data_export::Format;
#[derive(Clone, Copy)]
pub(super) enum Action {
    Scope(bool),
    Next,
    Back,
    Pick,
    Start,
    Close,
}
fn label(ui: &Ui, rect: Rect, text: &str, color: Color32, size: f32) {
    ui.painter().text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        font(ui.ctx(), size),
        color,
    );
}
pub(super) fn render(ui: &mut Ui, draft: &mut Draft) -> Option<Action> {
    let rect = ui.max_rect();
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    ui.painter().rect_filled(rect, 0., Color32::WHITE);
    ui.style_mut().override_font_id = Some(font(ui.ctx(), 13.));
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    let mut action = None;
    if ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
        action = Some(Action::Close);
    }
    if draft.scope {
        ui.painter().circle_filled(
            rect.min + vec2(25., 25.),
            14.,
            Color32::from_rgb(50, 95, 190),
        );
        ui.painter().text(
            rect.min + vec2(25., 25.),
            Align2::CENTER_CENTER,
            "?",
            font(ui.ctx(), 22.),
            Color32::WHITE,
        );
        label(
            ui,
            at(48., 12., 270., 26.),
            "你想导出表的全部记录吗?",
            Color32::BLACK,
            13.,
        );
        let count = draft.count;
        ui.add_enabled_ui(draft.all_available, |ui| {
            if button(ui, at(26., 60., 75., 23.), "全部记录", true)
                .on_hover_text(if draft.all_available {
                    "重新读取表中的全部记录，不受当前分页限制"
                } else {
                    "查询结果导出使用当前已加载的结果；请从打开表导出全部记录"
                })
                .clicked()
            {
                action = Some(Action::Scope(true));
            }
        });
        if button(
            ui,
            at(108., 60., 109., 23.),
            &format!("当前 {count} 条记录"),
            false,
        )
        .clicked()
        {
            action = Some(Action::Scope(false));
        }
        if button(ui, at(224., 60., 72., 23.), "取消", false).clicked() {
            action = Some(Action::Close);
        }
        return action;
    }
    let heading = match draft.step {
        0 => "向导可以让你指定导出数据的细节。你要使用哪一种导出格式？",
        1 => "指定导出文件的保存位置。",
        2 => "选择你要导出的字段。",
        3 => "指定导出选项，检查后开始导出。",
        _ => "导出记录并保存文件。",
    };
    label(
        ui,
        at(16., 18., 608., 30.),
        &format!("{heading}  ({}/5)", draft.step + 1),
        Color32::from_rgb(0, 119, 255),
        16.,
    );
    match draft.step {
        0 => {
            label(
                ui,
                at(20., 90., 200., 22.),
                "导出格式:",
                Color32::BLACK,
                13.,
            );
            for (i, format) in Format::ALL.into_iter().enumerate() {
                let row = at(20., 116. + i as f32 * 26., 590., 23.);
                let response = ui.interact(
                    row,
                    ui.id().with(("export-format", i)),
                    egui::Sense::click(),
                );
                let center = egui::pos2(row.left() + 6., row.center().y);
                ui.painter().circle_stroke(
                    center,
                    6.,
                    egui::Stroke::new(1., Color32::from_gray(80)),
                );
                if draft.options.format == format {
                    ui.painter()
                        .circle_filled(center, 3., Color32::from_gray(55));
                }
                label(
                    ui,
                    Rect::from_min_max(row.min + vec2(17., 0.), row.max),
                    format.label(),
                    Color32::BLACK,
                    13.,
                );
                if response.clicked() {
                    draft.options.format = format;
                    draft.options.delimiter = if format == Format::Csv { ',' } else { '\t' };
                    if !draft.path.is_empty() {
                        draft.path = PathBuf::from(&draft.path)
                            .with_extension(format.extension())
                            .to_string_lossy()
                            .into_owned();
                    }
                }
            }
        }
        1 => {
            label(
                ui,
                at(20., 85., 580., 23.),
                if draft.all {
                    "导出范围：全部记录（重新从数据库读取，包含所有分页）"
                } else {
                    "导出范围：当前显示记录（包含当前未保存的单元格编辑）"
                },
                Color32::BLACK,
                13.,
            );
            label(
                ui,
                at(20., 128., 100., 22.),
                "导出到文件:",
                Color32::BLACK,
                13.,
            );
            ui.put(
                at(20., 155., 520., 25.),
                TextEdit::singleline(&mut draft.path).desired_width(508.),
            );
            if button(ui, at(550., 155., 70., 25.), "浏览…", false).clicked() {
                action = Some(Action::Pick);
            }
            label(
                ui,
                at(20., 198., 600., 24.),
                draft.options.format.label(),
                Color32::from_gray(75),
                13.,
            );
        }
        2 => fields::render(ui, draft, rect),
        3 => {
            ui.scope_builder(egui::UiBuilder::new().max_rect(at(20.,85.,600.,290.)),|ui|{
                ui.label(format!("格式：{}",draft.options.format.label()));
                ui.label(format!("文件：{}",draft.path));
                ui.label(format!("字段：{}",draft.fields.iter().filter(|v|**v).count()));
                if matches!(draft.options.format,Format::Text|Format::Csv|Format::Html|Format::Xls|Format::Xlsx){ui.checkbox(&mut draft.options.header,"包含字段名");}
                if matches!(draft.options.format,Format::Text|Format::Csv){
                    ui.checkbox(&mut draft.options.bom,"UTF-8 BOM（便于 Excel 识别中文）");
                    ui.horizontal(|ui|{ui.label("分隔符：");for (text,c) in [("制表符",'\t'),("逗号",','),("分号",';'),("竖线",'|')]{ui.radio_value(&mut draft.options.delimiter,c,text);}});
                    ui.horizontal(|ui|{ui.label("NULL 标记：");ui.add(TextEdit::singleline(&mut draft.options.null_text).desired_width(130.));});
                }
                ui.add_space(12.);
                if draft.options.format==Format::Dbase {ui.label("DBF 使用 GBK；字段名最多 10 字节，文本最多 254 字节。超出限制会提示，不截断数据。");}
                if draft.options.format==Format::Xls {ui.label("XLS 每张工作表最多 65536 行、256 列；较大表请选择 XLSX 或 CSV。");}
                if draft.all {ui.label("全部记录读取数据库中已保存的数据；当前页未保存的编辑不会写入此文件。");}
            });
        }
        _ => {
            let (records, logs) = draft
                .control
                .read(|p| (p.records, p.logs.clone()))
                .unwrap_or_default();
            let status = if draft.closing {
                "正在取消，清理临时文件…"
            } else if draft.running() {
                "正在导出…"
            } else if draft.report.is_some() {
                "导出完成"
            } else {
                "导出已停止"
            };
            label(ui, at(20., 80., 600., 26.), status, Color32::BLACK, 14.);
            label(
                ui,
                at(20., 112., 600., 24.),
                &format!("已读取 {records} 条记录"),
                Color32::BLACK,
                13.,
            );
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(at(20., 150., 600., 235.)),
                |ui| {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &logs {
                                ui.label(line);
                            }
                        });
                },
            );
        }
    }
    if let Some(error) = &draft.error {
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(at(20., 393., 600., 43.)),
            |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                ui.colored_label(Color32::from_rgb(190, 35, 35), error);
            },
        );
    }
    let y = rect.height() - 40.;
    ui.painter()
        .rect_filled(at(0., y, rect.width(), 40.), 0., Color32::from_gray(240));
    ui.painter().hline(
        rect.x_range(),
        rect.top() + y,
        egui::Stroke::new(1., Color32::from_gray(208)),
    );
    if draft.step > 0
        && draft.step < 4
        && button(ui, at(358., y + 8., 76., 23.), "< 上一步", false).clicked()
    {
        action = Some(Action::Back);
    }
    if draft.step < 4 {
        if button(
            ui,
            at(446., y + 8., 74., 23.),
            if draft.step == 3 {
                "开始"
            } else {
                "下一步 >"
            },
            true,
        )
        .clicked()
        {
            action = Some(if draft.step == 3 {
                Action::Start
            } else {
                Action::Next
            });
        }
        if button(ui, at(524., y + 8., 31., 23.), ">>", false)
            .on_hover_text("跳到导出选项")
            .clicked()
        {
            if draft.validate().is_ok() {
                draft.step = 3;
            } else {
                draft.step = 1;
                draft.error = Some("请先选择导出文件".into());
            }
        }
    }
    if button(
        ui,
        at(562., y + 8., 70., 23.),
        if draft.step == 4 && !draft.running() {
            "关闭"
        } else {
            "取消"
        },
        false,
    )
    .clicked()
    {
        action = Some(Action::Close);
    }
    action
}
