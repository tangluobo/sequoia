use super::*;
use crate::native_dialog::{button, font};
use egui::{Align2, Color32, Rect, Sense, Stroke, vec2};
use tomato_core::workbench::Cell;
#[derive(Clone, Copy)]
pub(super) enum Action {
    Next,
    Back,
    Pick,
    Preview,
    OdbcSources,
    OdbcTables,
    Start,
    Confirmed,
    Close,
    SaveProfile,
    LoadProfile,
    SaveLog,
}
fn label(ui: &Ui, rect: Rect, text: &str, color: Color32, size: f32) {
    ui.painter().with_clip_rect(rect).text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        text,
        font(ui.ctx(), size),
        color,
    );
}
fn edit(ui: &mut Ui, rect: Rect, text: &mut String, password: bool) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.add_sized(
            rect.size(),
            egui::TextEdit::singleline(text)
                .password(password)
                .char_limit(65536)
                .font(font(ui.ctx(), 13.)),
        );
    });
}
fn multiline(ui: &mut Ui, rect: Rect, text: &mut String) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        egui::ScrollArea::both().show(ui, |ui| {
            ui.add_sized(
                rect.size(),
                egui::TextEdit::multiline(text)
                    .char_limit(1024 * 1024)
                    .font(font(ui.ctx(), 13.)),
            );
        });
    });
}
fn area(ui: &mut Ui, rect: Rect, draw: impl FnOnce(&mut Ui)) {
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), draw);
}
fn checkbox(ui: &mut Ui, rect: Rect, value: &mut bool, text: &str) {
    area(ui, rect, |ui| {
        ui.add_sized(rect.size(), egui::Checkbox::new(value, text));
    });
}
fn number(ui: &mut Ui, rect: Rect, value: &mut u64) {
    area(ui, rect, |ui| {
        ui.add_sized(
            rect.size(),
            egui::DragValue::new(value).range(1..=u64::MAX).speed(1.),
        );
    });
}
fn combo(
    ui: &mut Ui,
    rect: Rect,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut String,
    options: &[&str],
) {
    area(ui, rect, |ui| {
        egui::ComboBox::from_id_salt(id)
            .width(rect.width())
            .selected_text(value.as_str())
            .show_ui(ui, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                for &text in options {
                    ui.selectable_value(value, text.into(), text);
                }
            });
    });
}
fn short(value: &str) -> String {
    value.chars().take(160).collect()
}
pub(super) fn render(ui: &mut Ui, draft: &mut Draft) -> Option<Action> {
    ui.spacing_mut().interact_size.y = 22.;
    ui.spacing_mut().item_spacing.y = 0.;
    ui.spacing_mut().button_padding.y = 2.;
    let rect = ui.max_rect();
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    let mut action = None;
    ui.painter().rect_filled(rect, 0., Color32::WHITE);
    ui.style_mut().override_font_id = Some(font(ui.ctx(), 13.));
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
    if ui.input(|i| i.viewport().close_requested() || i.key_pressed(egui::Key::Escape)) {
        action = Some(Action::Close);
    }
    let title = match draft.step {
        0 => "这个向导允许你指定如何导入数据。你要选择哪种数据导入格式？",
        1 => "指定源文件，或连接 ODBC 数据源。",
        2 => "指定源数据格式、字符编码和记录范围。",
        3 => "选择源表并指定要导入的目标表。",
        4 => "映射源字段和目标字段，选择用于匹配记录的键字段。",
        5 => "选择如何将记录导入目标表。",
        6 => "指定导入选项，检查设置后开始导入。",
        _ if draft.report.as_ref().is_some_and(|r| {
            r.error.is_none() && !r.cancelled && !r.uncertain && r.failed == 0
        }) =>
        {
            "导入完成。"
        }
        _ if draft.report.is_some() => "导入已结束，请查看结果和日志。",
        _ if draft.error.is_some() && !draft.busy() => "导入未完成，请查看错误信息。",
        _ => "正在将记录导入数据库。",
    };
    label(
        ui,
        at(16., 12., 650., 34.),
        title,
        Color32::from_rgb(0, 119, 255),
        16.,
    );
    ui.add_enabled_ui(!draft.busy(), |ui| match draft.step {
        0 => {
            label(
                ui,
                at(20., 88., 300., 22.),
                "导入类型:",
                Color32::BLACK,
                13.,
            );
            let previous = draft.source.format;
            for (i, format) in Format::ALL.iter().enumerate() {
                area(ui, at(20., 114. + i as f32 * 26., 630., 24.), |ui| {
                    ui.radio_value(&mut draft.source.format, *format, format.label());
                });
            }
            if previous != draft.source.format {
                let tabular = matches!(
                    draft.source.format,
                    Format::Text | Format::Csv | Format::Excel
                );
                draft.source.header_row = usize::from(tabular);
                draft.source.first_row = if tabular { 2 } else { 1 };
                draft.source.encoding =
                    if matches!(draft.source.format, Format::Dbase | Format::Paradox) {
                        "自动".into()
                    } else {
                        "UTF-8".into()
                    };
                draft.delimiter = if draft.source.format == Format::Csv {
                    ",".into()
                } else {
                    "\\t".into()
                };
                draft.prepared = None;
                draft.targets.clear();
            }
        }
        1 => {
            if draft.source.format == Format::Odbc {
                odbc(ui, draft, rect, &mut action)
            } else {
                label(
                    ui,
                    at(20., 84., 620., 24.),
                    "源文件（可选择多个文件，每行一个）:",
                    Color32::BLACK,
                    13.,
                );
                multiline(ui, at(20., 114., 548., 245.), &mut draft.files);
                if button(ui, at(580., 114., 80., 24.), "浏览...", false).clicked() {
                    action = Some(Action::Pick)
                }
            }
        }
        2 => options(ui, draft, rect, &mut action),
        3 => targets(ui, draft, rect),
        4 => mappings(ui, draft, rect),
        5 => {
            let keys = draft
                .targets
                .iter()
                .filter(|t| t.selected)
                .all(|t| !t.create && t.fields.iter().any(|f| f.enabled && f.key));
            for (i, mode) in Mode::ALL.iter().enumerate() {
                area(ui, at(20., 90. + i as f32 * 31., 630., 26.), |ui| {
                    if ui
                        .add_enabled(
                            !mode.needs_key() || keys,
                            egui::RadioButton::new(draft.options.mode == *mode, mode.label()),
                        )
                        .clicked()
                    {
                        draft.options.mode = *mode;
                    }
                    if draft.options.mode == *mode && *mode == Mode::Copy {
                        draft.options.transaction = true
                    }
                });
            }
            let detail = match draft.options.mode {
                Mode::Append => "插入所有源记录；重复键或约束冲突按错误处理选项执行。",
                Mode::Update => "使用键字段匹配并更新已有记录；没有匹配项的源记录会跳过。",
                Mode::AppendUpdate => "使用键字段匹配，存在则更新，不存在则插入。",
                Mode::SkipDuplicates => "目标中已存在键值的记录会跳过，其余记录插入。",
                Mode::Delete => "删除键字段匹配的目标记录，不插入新记录。",
                Mode::Copy => "清空所选目标表的数据，然后插入源记录；使用单表事务。",
            };
            label(
                ui,
                at(20., 308., 640., 48.),
                detail,
                Color32::from_gray(70),
                13.,
            );
        }
        6 => {
            checkbox(
                ui,
                at(20., 86., 310., 25.),
                &mut draft.options.transaction,
                "单表事务（失败或取消时回滚本表）",
            );
            area(ui, at(350., 86., 300., 25.), |ui| {
                ui.add_enabled_ui(!draft.options.transaction, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("每批提交");
                        ui.add(
                            egui::DragValue::new(&mut draft.options.batch_size).range(1..=10000),
                        );
                        ui.label("条");
                    });
                });
            });
            checkbox(
                ui,
                at(20., 120., 620., 25.),
                &mut draft.options.continue_on_error,
                "遇到错误继续导入其他记录（错误记录单独回滚）",
            );
            checkbox(
                ui,
                at(20., 154., 620., 25.),
                &mut draft.options.binary_base64,
                "文本映射到二进制字段时使用 Base64（否则为十六进制）",
            );
            let records = draft.prepared.as_ref().map_or(0, |p| {
                draft
                    .targets
                    .iter()
                    .filter(|t| t.selected)
                    .map(|t| p.tables[t.source].rows)
                    .sum::<u64>()
            });
            label(
                ui,
                at(20., 194., 640., 24.),
                &format!(
                    "目标：{} / {}，{} 张表，源记录 {records} 条",
                    draft.connection.name,
                    draft.namespace,
                    draft.targets.iter().filter(|t| t.selected).count()
                ),
                Color32::BLACK,
                13.,
            );
            label(
                ui,
                at(20., 228., 640., 22.),
                "新建表结构预览（已有表保持原结构）:",
                Color32::BLACK,
                13.,
            );
            let mut sql = draft
                .targets
                .iter()
                .filter(|t| t.selected && t.create)
                .map(|t| {
                    service::create_sql(draft.connection.kind(), t)
                        .unwrap_or_else(|e| e.to_string())
                })
                .collect::<Vec<_>>()
                .join(";\n");
            if sql.is_empty() {
                sql = "使用已有目标表结构。".into()
            }
            area(ui, at(20., 254., 640., 135.), |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(&sql).font(font(ui.ctx(), 13.)))
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .selectable(true),
                        );
                    });
            });
        }
        _ => {}
    });
    if draft.step == 7 {
        progress(ui, draft, rect, &mut action)
    }
    if draft.busy() && draft.step != 7 {
        label(
            ui,
            at(20., 388., 640., 23.),
            "正在读取数据，请稍候…（可取消）",
            Color32::from_gray(80),
            13.,
        );
    }
    if let Some(error) = &draft.error {
        let r = at(20., 410., 640., 25.);
        label(ui, r, &short(error), Color32::from_rgb(185, 35, 35), 12.);
        ui.interact(r, ui.id().with("import-error"), Sense::hover())
            .on_hover_text(error);
    }
    ui.painter()
        .rect_filled(at(0., 440., 680., 40.), 0., Color32::from_gray(240));
    ui.painter().line_segment(
        [
            at(0., 440., 680., 1.).left_top(),
            at(0., 440., 680., 1.).right_top(),
        ],
        Stroke::new(1., Color32::from_gray(210)),
    );
    ui.add_enabled_ui(!draft.busy(), |ui| {
        if button(ui, at(8., 450., 84., 23.), "加载配置", false).clicked() {
            action = Some(Action::LoadProfile)
        }
        if draft.step > 0
            && draft.step < 7
            && button(ui, at(100., 450., 84., 23.), "保存配置", false).clicked()
        {
            action = Some(Action::SaveProfile)
        }
        if draft.step > 0
            && draft.step < 7
            && button(ui, at(432., 450., 80., 23.), "< 上一步", false).clicked()
        {
            action = Some(Action::Back)
        }
        if draft.step < 6 {
            if button(ui, at(520., 450., 74., 23.), "下一步 >", true).clicked() {
                action = Some(Action::Next)
            }
        } else if draft.step == 6 && button(ui, at(520., 450., 74., 23.), "开始", true).clicked()
        {
            action = Some(Action::Start)
        }
    });
    if button(
        ui,
        at(600., 450., 72., 23.),
        if draft.step == 7 && !draft.busy() {
            "关闭"
        } else {
            "取消"
        },
        false,
    )
    .clicked()
    {
        action = Some(Action::Close)
    }
    if draft.confirming {
        egui::Window::new("确认导入操作")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0., 0.))
            .show(ui.ctx(), |ui| {
                ui.set_width(400.);
                ui.label(format!(
                    "将对以下目标执行“{}”：",
                    draft.options.mode.label()
                ));
                for t in draft.targets.iter().filter(|t| t.selected) {
                    ui.label(format!("{}.{}", t.table.schema, t.table.name));
                }
                ui.horizontal(|ui| {
                    if ui.button("开始导入").clicked() {
                        action = Some(Action::Confirmed)
                    }
                    if ui.button("返回").clicked() {
                        draft.confirming = false;
                    }
                });
            });
    }
    action
}
fn odbc(ui: &mut Ui, d: &mut Draft, rect: Rect, action: &mut Option<Action>) {
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    area(ui, at(20., 78., 300., 26.), |ui| {
        ui.horizontal(|ui| {
            ui.label("驱动位数:");
            ui.radio_value(&mut d.source.odbc.use_32bit, false, "64 位");
            #[cfg(windows)]
            ui.radio_value(&mut d.source.odbc.use_32bit, true, "32 位");
        });
    });
    if button(ui, at(438., 78., 105., 24.), "列出数据源", false).clicked() {
        *action = Some(Action::OdbcSources)
    }
    if button(ui, at(550., 78., 110., 24.), "连接并列出表", false).clicked() {
        *action = Some(Action::OdbcTables)
    }
    label(
        ui,
        at(20., 114., 85., 23.),
        "数据源名称:",
        Color32::BLACK,
        13.,
    );
    edit(ui, at(110., 114., 300., 23.), &mut d.source.odbc.dsn, false);
    area(ui, at(420., 114., 240., 23.), |ui| {
        egui::ComboBox::from_id_salt("import-odbc-dsn")
            .width(230.)
            .selected_text("已配置的数据源")
            .show_ui(ui, |ui| {
                for name in &d.odbc.dsns {
                    ui.selectable_value(&mut d.source.odbc.dsn, name.clone(), name);
                }
            });
    });
    label(ui, at(20., 146., 85., 23.), "用户名:", Color32::BLACK, 13.);
    edit(
        ui,
        at(110., 146., 235., 23.),
        &mut d.source.odbc.user,
        false,
    );
    label(ui, at(365., 146., 55., 23.), "密码:", Color32::BLACK, 13.);
    edit(
        ui,
        at(420., 146., 240., 23.),
        &mut d.source.odbc.password,
        true,
    );
    label(
        ui,
        at(20., 178., 640., 22.),
        "连接字符串（可替代数据源名称，不写入导入配置）:",
        Color32::BLACK,
        13.,
    );
    edit(
        ui,
        at(20., 204., 640., 24.),
        &mut d.source.odbc.connection_string,
        true,
    );
    label(
        ui,
        at(20., 238., 640., 22.),
        "源查询（可选，填写后使用单条 SELECT 查询）:",
        Color32::BLACK,
        13.,
    );
    multiline(ui, at(20., 262., 640., 56.), &mut d.source.odbc.query);
    area(ui, at(20., 326., 640., 77.), |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            if d.odbc.odbc_tables.is_empty() {
                ui.label(format!("可用驱动：{}", d.odbc.drivers.join("；")));
            }
            for table in &d.odbc.odbc_tables {
                let mut selected = d.source.odbc.tables.iter().any(|t| {
                    t.catalog == table.catalog && t.schema == table.schema && t.name == table.name
                });
                if ui
                    .checkbox(&mut selected, format!("{}.{}", table.schema, table.name))
                    .changed()
                {
                    if selected {
                        d.source.odbc.tables.push(table.clone())
                    } else {
                        d.source.odbc.tables.retain(|t| {
                            !(t.catalog == table.catalog
                                && t.schema == table.schema
                                && t.name == table.name)
                        });
                    }
                }
            }
        });
    });
}
fn options(ui: &mut Ui, d: &mut Draft, rect: Rect, action: &mut Option<Action>) {
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    let delimited = matches!(d.source.format, Format::Text | Format::Csv);
    let sheet = matches!(d.source.format, Format::Excel | Format::Access);
    label(ui, at(20., 76., 85., 24.), "字符编码:", Color32::BLACK, 13.);
    combo(
        ui,
        at(110., 76., 220., 24.),
        "import-encoding",
        &mut d.source.encoding,
        &[
            "自动",
            "UTF-8",
            "UTF-16LE",
            "UTF-16BE",
            "GB18030",
            "GBK",
            "Big5",
            "Shift_JIS",
            "windows-1252",
            "IBM866",
        ],
    );
    if delimited || d.source.format == Format::Excel {
        label(
            ui,
            at(20., 110., 85., 24.),
            "字段名行:",
            Color32::BLACK,
            13.,
        );
        area(ui, at(110., 110., 85., 24.), |ui| {
            ui.add_sized(
                vec2(85., 24.),
                egui::DragValue::new(&mut d.source.header_row).range(0..=1000000),
            );
        });
        label(
            ui,
            at(205., 110., 150., 24.),
            "0 表示没有字段名行",
            Color32::from_gray(100),
            12.,
        );
    }
    label(ui, at(20., 144., 85., 24.), "开始行:", Color32::BLACK, 13.);
    number(ui, at(110., 144., 90., 24.), &mut d.source.first_row);
    label(ui, at(230., 144., 70., 24.), "结束行:", Color32::BLACK, 13.);
    edit(ui, at(306., 144., 110., 24.), &mut d.last_row, false);
    label(
        ui,
        at(426., 144., 180., 24.),
        "留空表示到文件末尾",
        Color32::from_gray(100),
        12.,
    );
    if delimited {
        label(
            ui,
            at(20., 178., 85., 24.),
            "字段分隔符:",
            Color32::BLACK,
            13.,
        );
        edit(ui, at(110., 178., 90., 24.), &mut d.delimiter, false);
        label(
            ui,
            at(230., 178., 80., 24.),
            "文本限定符:",
            Color32::BLACK,
            13.,
        );
        edit(ui, at(320., 178., 60., 24.), &mut d.quote, false);
        label(
            ui,
            at(405., 178., 70., 24.),
            "NULL 文本:",
            Color32::BLACK,
            13.,
        );
        edit(ui, at(486., 178., 110., 24.), &mut d.null_value, false);
        checkbox(
            ui,
            at(20., 212., 210., 24.),
            &mut d.source.trim,
            "移除字段首尾空格",
        );
        checkbox(
            ui,
            at(250., 212., 210., 24.),
            &mut d.source.skip_empty,
            "跳过空记录",
        );
    } else if d.source.format == Format::Json {
        label(
            ui,
            at(20., 178., 100., 24.),
            "记录数组路径:",
            Color32::BLACK,
            13.,
        );
        edit(
            ui,
            at(130., 178., 470., 24.),
            &mut d.source.json_path,
            false,
        );
        label(
            ui,
            at(20., 210., 640., 24.),
            "留空表示根数组或根对象；嵌套数组例如 data.rows 或 /data/rows。",
            Color32::from_gray(90),
            12.,
        );
    } else if d.source.format == Format::Xml {
        label(
            ui,
            at(20., 178., 100., 24.),
            "记录节点:",
            Color32::BLACK,
            13.,
        );
        edit(ui, at(130., 178., 470., 24.), &mut d.source.xml_row, false);
        label(
            ui,
            at(20., 210., 640., 24.),
            "留空自动识别 record、row 或根元素的直接子元素。",
            Color32::from_gray(90),
            12.,
        );
    } else if d.source.format == Format::Access {
        label(
            ui,
            at(20., 178., 100., 24.),
            "数据库密码:",
            Color32::BLACK,
            13.,
        );
        edit(
            ui,
            at(130., 178., 300., 24.),
            &mut d.source.access_password,
            true,
        );
    }
    label(
        ui,
        at(20., 246., 85., 24.),
        "日期顺序:",
        Color32::BLACK,
        13.,
    );
    combo(
        ui,
        at(110., 246., 110., 24.),
        "import-date-order",
        &mut d.source.date_order,
        &["YMD", "DMY", "MDY"],
    );
    label(
        ui,
        at(250., 246., 110., 24.),
        "日期分隔符:",
        Color32::BLACK,
        13.,
    );
    edit(
        ui,
        at(360., 246., 60., 24.),
        &mut d.source.date_separator,
        false,
    );
    if sheet {
        label(
            ui,
            at(20., 280., 490., 22.),
            "只读取这些源表/工作表（每行一个，留空为全部）:",
            Color32::BLACK,
            12.,
        );
        multiline(ui, at(20., 306., 480., 70.), &mut d.selected_tables);
    }
    if button(ui, at(540., 280., 120., 24.), "预览源数据", false).clicked() {
        *action = Some(Action::Preview)
    }
    if d.prepared.is_some() && !sheet {
        preview(ui, d, at(20., 310., 640., 88.));
    }
}
fn targets(ui: &mut Ui, d: &mut Draft, rect: Rect) {
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    label(
        ui,
        at(20., 76., 640., 24.),
        "选择        源数据表 / 记录数                  目标类型          目标表",
        Color32::BLACK,
        13.,
    );
    area(ui, at(20., 106., 640., 148.), |ui| {
        egui::ScrollArea::vertical().show_rows(ui, 27., d.targets.len(), |ui, range| {
            for index in range {
                let (r, _) = ui.allocate_exact_size(vec2(620., 27.), Sense::hover());
                let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(r));
                let ui = &mut row_ui;
                let target = &mut d.targets[index];
                let source = &d.prepared.as_ref().unwrap().tables[target.source];
                checkbox(
                    ui,
                    Rect::from_min_size(r.min, vec2(28., 25.)),
                    &mut target.selected,
                    "",
                );
                label(
                    ui,
                    Rect::from_min_size(r.min + vec2(32., 0.), vec2(238., 25.)),
                    &format!("{}  ({})", source.name, source.rows),
                    Color32::BLACK,
                    13.,
                );
                area(
                    ui,
                    Rect::from_min_size(r.min + vec2(275., 1.), vec2(94., 24.)),
                    |ui| {
                        egui::ComboBox::from_id_salt(("import-target-kind", index))
                            .width(90.)
                            .selected_text(if target.create {
                                "新建表"
                            } else {
                                "已有表"
                            })
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_value(&mut target.create, true, "新建表")
                                    .changed()
                                {
                                    target.expected = None;
                                }
                                if ui
                                    .selectable_value(&mut target.create, false, "已有表")
                                    .changed()
                                {
                                    target.expected = None;
                                }
                            });
                    },
                );
                let field = Rect::from_min_size(r.min + vec2(380., 1.), vec2(228., 24.));
                if target.create {
                    edit(ui, field, &mut target.table.name, false)
                } else {
                    area(ui, field, |ui| {
                        egui::ComboBox::from_id_salt(("import-target-table", index))
                            .width(225.)
                            .selected_text(&target.table.name)
                            .show_ui(ui, |ui| {
                                for table in &d.known {
                                    if ui
                                        .selectable_label(
                                            table.table == target.table,
                                            &table.table.name,
                                        )
                                        .clicked()
                                    {
                                        target.table = table.table.clone();
                                        target.expected = None;
                                    }
                                }
                            });
                    });
                }
            }
        });
    });
    area(ui, at(20., 265., 640., 25.), |ui| {
        if let Some(prepared) = &d.prepared {
            egui::ComboBox::from_id_salt("import-preview-table")
                .width(300.)
                .selected_text(&prepared.tables[d.preview.min(prepared.tables.len() - 1)].name)
                .show_ui(ui, |ui| {
                    for (i, t) in prepared.tables.iter().enumerate() {
                        ui.selectable_value(&mut d.preview, i, &t.name);
                    }
                });
        }
    });
    preview(ui, d, at(20., 298., 640., 103.));
}
fn preview(ui: &mut Ui, d: &Draft, rect: Rect) {
    let Some(p) = &d.prepared else { return };
    let Some(table) = p.tables.get(d.preview) else {
        return;
    };
    area(ui, rect, |ui| {
        egui::ScrollArea::both().show(ui, |ui| {
            let width = (table.fields.len() as f32 * 150.).max(rect.width());
            let (header, _) = ui.allocate_exact_size(vec2(width, 23.), Sense::hover());
            let clip = ui.clip_rect();
            let first = ((clip.left() - header.left()) / 150.).floor().max(0.) as usize;
            let last =
                (((clip.right() - header.left()) / 150.).ceil() as usize).min(table.fields.len());
            ui.painter()
                .rect_filled(header, 0., Color32::from_gray(240));
            for (col, field) in table.fields.iter().enumerate().take(last).skip(first) {
                let r = Rect::from_min_size(
                    header.min + vec2(col as f32 * 150. + 4., 0.),
                    vec2(143., 23.),
                );
                label(ui, r, &field.name, Color32::BLACK, 12.);
            }
            for (index, row) in table.preview.iter().enumerate() {
                let (r, _) = ui.allocate_exact_size(vec2(width, 23.), Sense::hover());
                if !r.intersects(clip) {
                    continue;
                }
                if index % 2 == 1 {
                    ui.painter().rect_filled(r, 0., Color32::from_gray(250));
                }
                for col in first..last {
                    let value = row.get(col).map_or_else(
                        || "NULL".into(),
                        |cell| match cell {
                            Cell::Text(s) => short(s),
                            Cell::Binary(b) => b
                                .iter()
                                .take(80)
                                .map(|byte| format!("{byte:02x}"))
                                .collect(),
                            _ => cell.display(),
                        },
                    );
                    let cell = Rect::from_min_size(
                        r.min + vec2(col as f32 * 150. + 4., 0.),
                        vec2(143., 23.),
                    );
                    label(ui, cell, &short(&value), Color32::BLACK, 12.);
                }
                ui.painter().hline(
                    r.x_range(),
                    r.bottom(),
                    Stroke::new(1., Color32::from_gray(225)),
                );
            }
        });
    });
}
fn mappings(ui: &mut Ui, d: &mut Draft, rect: Rect) {
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    area(ui, at(20., 76., 640., 25.), |ui| {
        egui::ComboBox::from_id_salt("import-field-table")
            .width(360.)
            .selected_text(
                d.targets
                    .get(d.selected)
                    .map_or("", |t| t.table.name.as_str()),
            )
            .show_ui(ui, |ui| {
                for (i, t) in d.targets.iter().enumerate().filter(|(_, t)| t.selected) {
                    ui.selectable_value(&mut d.selected, i, &t.table.name);
                }
            });
    });
    let Some(target) = d.targets.get_mut(d.selected) else {
        return;
    };
    let source = &d.prepared.as_ref().unwrap().tables[target.source];
    area(ui, at(20., 112., 640., 25.), |ui| {
        ui.horizontal(|ui| {
            if ui.button("按名称匹配").clicked() {
                for f in &mut target.fields {
                    if let Some(i) = source
                        .fields
                        .iter()
                        .position(|s| s.name.eq_ignore_ascii_case(&f.target))
                    {
                        f.source = Some(i);
                        f.source_name = Some(source.fields[i].name.clone());
                        f.enabled = true;
                    }
                }
            }
            if ui.button("按顺序匹配").clicked() {
                for (i, f) in target.fields.iter_mut().enumerate() {
                    f.source = if i < source.fields.len() {
                        Some(i)
                    } else {
                        None
                    };
                    f.source_name = f.source.map(|i| source.fields[i].name.clone());
                    f.enabled = f.source.is_some();
                }
            }
            if ui.button("取消全部匹配").clicked() {
                for f in &mut target.fields {
                    f.enabled = false;
                }
            }
            if target.create && ui.button("添加目标字段").clicked() {
                target.fields.push(Mapping {
                    enabled: true,
                    source: None,
                    source_name: None,
                    default: Some(String::new()),
                    target: format!("field{}", target.fields.len() + 1),
                    data_type: DataType::Text,
                    nullable: true,
                    key: false,
                });
            }
        });
    });
    area(ui, at(20., 148., 640., 252.), |ui| {
        egui::ScrollArea::both().show(ui, |ui| {
            let width = 850.;
            let (header, _) = ui.allocate_exact_size(vec2(width, 25.), Sense::hover());
            ui.painter()
                .rect_filled(header, 0., Color32::from_gray(240));
            for (x, w, text) in [
                (0., 32., "选"),
                (34., 32., "键"),
                (70., 170., "源字段"),
                (245., 170., "目标字段"),
                (420., 140., "数据类型"),
                (570., 70., "允许空"),
                (650., 190., "默认值"),
            ] {
                label(
                    ui,
                    Rect::from_min_size(header.min + vec2(x, 0.), vec2(w, 25.)),
                    text,
                    Color32::BLACK,
                    12.,
                );
            }
            egui::ScrollArea::vertical().show_rows(ui, 26., target.fields.len(), |ui, range| {
                for index in range {
                    let (r, _) = ui.allocate_exact_size(vec2(width, 26.), Sense::hover());
                    let mut row_ui = ui.new_child(egui::UiBuilder::new().max_rect(r));
                    let ui = &mut row_ui;
                    let f = &mut target.fields[index];
                    checkbox(
                        ui,
                        Rect::from_min_size(r.min, vec2(28., 25.)),
                        &mut f.enabled,
                        "",
                    );
                    checkbox(
                        ui,
                        Rect::from_min_size(r.min + vec2(35., 0.), vec2(28., 25.)),
                        &mut f.key,
                        "",
                    );
                    area(
                        ui,
                        Rect::from_min_size(r.min + vec2(70., 1.), vec2(165., 24.)),
                        |ui| {
                            egui::ComboBox::from_id_salt((
                                "import-source-field",
                                target.source,
                                index,
                            ))
                            .width(160.)
                            .selected_text(
                                f.source
                                    .and_then(|i| source.fields.get(i))
                                    .map_or("默认值 / 不映射", |s| s.name.as_str()),
                            )
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(f.source.is_none(), "默认值 / 不映射")
                                    .clicked()
                                {
                                    f.source = None;
                                    f.source_name = None;
                                }
                                for (i, s) in source.fields.iter().enumerate() {
                                    if ui.selectable_label(f.source == Some(i), &s.name).clicked() {
                                        f.source = Some(i);
                                        f.source_name = Some(s.name.clone());
                                    }
                                }
                            });
                        },
                    );
                    let rname = Rect::from_min_size(r.min + vec2(245., 1.), vec2(165., 24.));
                    if target.create {
                        edit(ui, rname, &mut f.target, false)
                    } else if let Some(def) = &target.expected {
                        area(ui, rname, |ui| {
                            egui::ComboBox::from_id_salt((
                                "import-dest-field",
                                target.source,
                                index,
                            ))
                            .width(160.)
                            .selected_text(&f.target)
                            .show_ui(ui, |ui| {
                                for c in def.columns.iter().filter(|c| !c.generated) {
                                    if ui
                                        .selectable_value(&mut f.target, c.name.clone(), &c.name)
                                        .changed()
                                    {
                                        f.data_type = service::column_type(c);
                                        f.nullable = c.nullable;
                                    }
                                }
                            });
                        });
                    }
                    area(
                        ui,
                        Rect::from_min_size(r.min + vec2(420., 1.), vec2(140., 24.)),
                        |ui| {
                            ui.add_enabled_ui(target.create, |ui| {
                                egui::ComboBox::from_id_salt(("import-type", target.source, index))
                                    .width(135.)
                                    .selected_text(f.data_type.label())
                                    .show_ui(ui, |ui| {
                                        for typ in [
                                            DataType::Text,
                                            DataType::Integer,
                                            DataType::Decimal {
                                                precision: 38,
                                                scale: 12,
                                            },
                                            DataType::Real,
                                            DataType::Boolean,
                                            DataType::Date,
                                            DataType::DateTime,
                                            DataType::Binary,
                                        ] {
                                            let label = typ.label();
                                            ui.selectable_value(&mut f.data_type, typ, label);
                                        }
                                        if let DataType::Decimal { precision, scale } =
                                            &mut f.data_type
                                        {
                                            ui.horizontal(|ui| {
                                                ui.label("精度");
                                                ui.add(
                                                    egui::DragValue::new(precision).range(1..=38),
                                                );
                                                ui.label("小数位");
                                                ui.add(
                                                    egui::DragValue::new(scale)
                                                        .range(0..=*precision),
                                                );
                                            });
                                        }
                                    });
                            });
                        },
                    );
                    area(
                        ui,
                        Rect::from_min_size(r.min + vec2(578., 0.), vec2(54., 25.)),
                        |ui| {
                            ui.add_enabled_ui(target.create, |ui| {
                                ui.checkbox(&mut f.nullable, "");
                            });
                        },
                    );
                    if f.source.is_none() {
                        let mut constant = f.default.is_some();
                        checkbox(
                            ui,
                            Rect::from_min_size(r.min + vec2(650., 0.), vec2(26., 25.)),
                            &mut constant,
                            "",
                        );
                        if constant {
                            edit(
                                ui,
                                Rect::from_min_size(r.min + vec2(680., 1.), vec2(150., 24.)),
                                f.default.get_or_insert_with(String::new),
                                false,
                            );
                        } else {
                            f.default = None;
                            label(
                                ui,
                                Rect::from_min_size(r.min + vec2(680., 0.), vec2(150., 25.)),
                                "使用数据库默认值",
                                Color32::from_gray(120),
                                12.,
                            );
                        }
                    } else {
                        label(
                            ui,
                            Rect::from_min_size(r.min + vec2(650., 0.), vec2(180., 25.)),
                            "使用源字段值",
                            Color32::from_gray(120),
                            12.,
                        );
                    }
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom(),
                        Stroke::new(1., Color32::from_gray(225)),
                    );
                }
            });
        });
    });
}
fn progress(ui: &mut Ui, d: &Draft, rect: Rect, action: &mut Option<Action>) {
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    let processed = d.control.read(|p| p.records).unwrap_or_default();
    let total = d.prepared.as_ref().map_or(0, |p| {
        d.targets
            .iter()
            .filter(|t| t.selected)
            .map(|t| p.tables[t.source].rows)
            .sum::<u64>()
    });
    label(
        ui,
        at(20., 80., 640., 24.),
        &format!("已处理 {processed} / {total} 条源记录"),
        Color32::BLACK,
        13.,
    );
    area(ui, at(20., 115., 640., 22.), |ui| {
        ui.add_sized(
            vec2(640., 22.),
            egui::ProgressBar::new(if total == 0 {
                0.
            } else {
                processed as f32 / total as f32
            })
            .show_percentage(),
        );
    });
    if let Some(r) = &d.report {
        label(
            ui,
            at(20., 150., 640., 24.),
            &format!(
                "插入 {}，更新 {}，删除 {}，跳过 {}，失败 {}",
                r.inserted, r.updated, r.deleted, r.skipped, r.failed
            ),
            Color32::BLACK,
            13.,
        );
        label(
            ui,
            at(20., 180., 640., 24.),
            &format!(
                "已提交变更 {}{}",
                r.committed,
                if r.uncertain {
                    "；部分提交结果未确认"
                } else if r.cancelled {
                    "；已取消"
                } else {
                    ""
                }
            ),
            Color32::BLACK,
            13.,
        );
        if button(ui, at(540., 208., 120., 24.), "保存日志", false).clicked() {
            *action = Some(Action::SaveLog)
        }
    }
    area(ui, at(20., 242., 640., 150.), |ui| {
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .show(ui, |ui| {
                d.control.read(|p| {
                    for line in &p.logs {
                        ui.label(line);
                    }
                });
            });
    });
}
