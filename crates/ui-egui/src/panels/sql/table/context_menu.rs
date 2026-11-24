//! One menu for data cells and frozen row selectors, matching TableDataView.java.
use super::*;
use tomato_core::{result_filter::Operator, result_transfer::Format};
#[derive(Clone, Copy)]
pub(super) enum Copy {
    Plain,
    Insert,
    Update,
    Json(bool),
    Comments,
    Tsv(bool, bool),
}
pub(super) enum Action {
    Set(Cell),
    Delete,
    Copy(Copy),
    Paste,
    Export,
    Sort(usize, bool, bool),
    RemoveSort(usize),
    Filter(usize, Operator),
    CustomFilter(usize),
    RemoveFilter(usize),
    ClearFilters,
    Clear,
    Visible(usize, bool),
    Refresh,
}
fn item(ui: &mut Ui, enabled: bool, label: &str) -> bool {
    ui.add_enabled(enabled, crate::compact_menu::Item::new(label).table())
        .clicked()
}
fn detail(ui: &mut Ui, enabled: bool, label: &str, icon: Option<usize>) -> bool {
    let texture=icon.map(|index|{
        type Icons=std::sync::Arc<(f32,Vec<egui::TextureHandle>)>;
        let key=egui::Id::new("original-table-context-icons");let scale=ui.ctx().pixels_per_point();
        let icons=ui.ctx().data(|data|data.get_temp::<Icons>(key)).filter(|icons|icons.0==scale).unwrap_or_else(||{
            let mut textures=Vec::new();
            for (index,paths) in [
                "<path d='M3 1h2v10h3l-4 4-4-4h3z' fill='#42A5F5'/><path d='M0 1h8v1.5H0z M0 5h7v1.5H0z M0 9h6v1.5H0z M0 13h5v1.5H0z' fill='#777777' transform='translate(10 0)'/>",
                "<path d='M1 2h14L9.5 8v5l-3 2V8z' fill='#42A5F5'/>"
            ].into_iter().enumerate(){
                let svg=format!("<svg xmlns='http://www.w3.org/2000/svg' width='18' height='16' viewBox='0 0 18 16'>{paths}</svg>");
                let tree=resvg::usvg::Tree::from_data(svg.as_bytes(),&resvg::usvg::Options::default()).expect("static Java menu SVG");
                let mut pixels=resvg::tiny_skia::Pixmap::new((18.*scale).ceil() as u32,(16.*scale).ceil() as u32).expect("menu icon");
                resvg::render(&tree,resvg::tiny_skia::Transform::from_scale(scale,scale),&mut pixels.as_mut());
                textures.push(ui.ctx().load_texture(format!("original-table-menu-{index}"),egui::ColorImage::from_rgba_premultiplied([pixels.width() as usize,pixels.height() as usize],pixels.data()),egui::TextureOptions::LINEAR));
            }
            let icons=std::sync::Arc::new((scale,textures));ui.ctx().data_mut(|data|data.insert_temp(key,icons.clone()));icons
        });icons.1[index].id()
    });
    ui.add_enabled(
        enabled,
        crate::compact_menu::Item::new(label)
            .table()
            .icon_column()
            .icon(texture),
    )
    .clicked()
}
pub(super) fn style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.item_spacing = egui::Vec2::ZERO;
    style.spacing.interact_size.y = 20.;
    style.spacing.button_padding = egui::vec2(4., 3.);
    style.spacing.menu_margin = egui::Margin {
        left: 0,
        right: 0,
        top: 2,
        bottom: 2,
    };
    style.visuals.menu_corner_radius = egui::CornerRadius::ZERO;
    style.visuals.disabled_alpha = 0.4;
    style.visuals.override_text_color = None;
    style.visuals.widgets.inactive.fg_stroke.color = Color32::from_gray(51);
    style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_gray(51);
    style.visuals.widgets.hovered.fg_stroke.color = Color32::WHITE;
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0, 150, 201);
    style.visuals.widgets.open.fg_stroke.color = Color32::WHITE;
    style.visuals.widgets.open.weak_bg_fill = Color32::from_rgb(0, 150, 201);
    style.visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(64),
    };
}
pub(super) fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::WHITE)
        .stroke(egui::Stroke::new(1., Color32::from_gray(189)))
        .inner_margin(egui::Margin {
            left: 0,
            right: 0,
            top: 2,
            bottom: 2,
        })
        .corner_radius(0.)
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(64),
        })
}
fn separator(ui: &mut Ui) {
    let width = if ui.is_sizing_pass() {
        0.
    } else {
        ui.available_width()
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 3.), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(1., Color32::from_gray(204)),
    );
}
pub(super) fn show(ui: &mut Ui, state: &Workbench, writable: bool) -> Option<Action> {
    let mut action = None;
    style(ui.style_mut());
    ui.set_min_width(112.);
    let selected = state.table_view.cells.iter().next().is_some();
    if item(ui, writable && selected, "设置为空字符串") {
        action = Some(Action::Set(Cell::Text(String::new())));
    }
    if item(ui, writable && selected, "设置为 NULL") {
        action = Some(Action::Set(Cell::Null));
    }
    separator(ui);
    let primary = state
        .definition
        .as_ref()
        .or_else(|| state.page.as_ref().map(|page| &page.definition))
        .is_some_and(|definition| {
            definition
                .columns
                .iter()
                .any(|column| column.primary_key > 0)
        });
    if ui
        .add_enabled(
            writable && primary && !state.selected.is_empty(),
            crate::compact_menu::Item::new("删除 记录").table().strong(),
        )
        .clicked()
    {
        action = Some(Action::Delete);
    }
    if item(ui, selected, "复制") {
        action = Some(Action::Copy(Copy::Plain));
    }
    ui.add_enabled_ui(selected, |ui| {
        ui.menu_button("复制为", |ui| {
            if item(ui, true, "Insert 语句") {
                action = Some(Action::Copy(Copy::Insert));
            }
            if item(ui, primary, "Update 语句") {
                action = Some(Action::Copy(Copy::Update));
            }
            separator(ui);
            ui.menu_button("JSON 格式", |ui| {
                if item(ui, true, "原字段") {
                    action = Some(Action::Copy(Copy::Json(false)));
                }
                if item(ui, true, "字段转驼峰") {
                    action = Some(Action::Copy(Copy::Json(true)));
                }
            });
            if item(ui, true, "注释：值") {
                action = Some(Action::Copy(Copy::Comments));
            }
            separator(ui);
            for (label, headers, data) in [
                ("制表符分隔值（数据）", false, true),
                ("制表符分隔值（字段名）", true, false),
                ("制表符分隔值（字段名和数据）", true, true),
            ] {
                if item(ui, true, label) {
                    action = Some(Action::Copy(Copy::Tsv(headers, data)));
                }
            }
            if action.is_some() {
                ui.close();
            }
        });
    });
    if item(ui, writable && state.table_view.paste_available, "粘贴") {
        action = Some(Action::Paste);
    }
    if item(ui, state.page.is_some(), "保存数据为...") {
        action = Some(Action::Export);
    }
    separator(ui);
    let column = (!state.table_view.selector_focus)
        .then_some(state.table_view.focus)
        .flatten()
        .map(|cell| cell.1);
    ui.menu_button(
        if state.table_view.projection.sorts.is_empty() {
            egui::RichText::new("排序")
        } else {
            egui::RichText::new("排序").strong()
        },
        |ui| {
            for (label, descending, append) in [
                ("升序排序", false, false),
                ("降序排序", true, false),
                ("排序", false, true),
            ] {
                if detail(ui, column.is_some(), label, append.then_some(0)) {
                    action = column.map(|column| Action::Sort(column, descending, append));
                }
            }
            separator(ui);
            if detail(
                ui,
                column.is_some_and(|column| {
                    state
                        .table_view
                        .projection
                        .sorts
                        .iter()
                        .any(|sort| sort.0 == column)
                }),
                "移除排序",
                None,
            ) {
                action = column.map(Action::RemoveSort);
            }
            if action.is_some() {
                ui.close();
            }
        },
    );
    ui.menu_button(
        if state.table_view.projection.filters.is_empty() {
            egui::RichText::new("筛选")
        } else {
            egui::RichText::new("筛选").strong()
        },
        |ui| {
            for (label, operator) in [
                ("字段 = 值", Operator::Equal),
                ("字段 != 值", Operator::NotEqual),
                ("字段类似值", Operator::Like),
                ("字段不类似值", Operator::NotLike),
                ("字段 < 值", Operator::Less),
                ("字段 > 值", Operator::Greater),
            ] {
                if detail(ui, column.is_some(), label, None) {
                    action = column.map(|column| Action::Filter(column, operator));
                }
            }
            separator(ui);
            for label in ["自定义筛选", "筛选"] {
                if detail(ui, column.is_some(), label, (label == "筛选").then_some(1)) {
                    action = column.map(Action::CustomFilter);
                }
            }
            separator(ui);
            if detail(
                ui,
                column.is_some_and(|column| {
                    state.table_view.projection.filters.contains_key(&column)
                }),
                "移除筛选",
                None,
            ) {
                action = column.map(Action::RemoveFilter);
            }
            if detail(
                ui,
                !state.table_view.projection.filters.is_empty(),
                "移除全部筛选",
                None,
            ) {
                action = Some(Action::ClearFilters);
            }
            if action.is_some() {
                ui.close();
            }
        },
    );
    if item(ui, true, "移除全部排序及筛选") {
        action = Some(Action::Clear);
    }
    separator(ui);
    ui.menu_button("显示", |ui| {
        if let Some(page) = &state.page {
            let height = (ui.ctx().content_rect().height() - 24.).clamp(40., 600.);
            egui::ScrollArea::vertical()
                .max_height(height)
                .auto_shrink([true, true])
                .show_rows(ui, 20., page.definition.columns.len(), |ui, range| {
                    for column in range {
                        let field = &page.definition.columns[column];
                        let mut visible = !state.table_view.projection.hidden.contains(&column);
                        if ui.checkbox(&mut visible, &field.name).changed() {
                            action = Some(Action::Visible(column, visible));
                        }
                    }
                });
        }
    });
    if item(ui, true, "刷新") {
        action = Some(Action::Refresh);
    }
    if action.is_some() {
        ui.close();
    }
    action
}
pub(super) fn format<'a>(kind: Copy, table: &'a str, keys: &'a [String]) -> Format<'a> {
    match kind {
        Copy::Insert => Format::Insert { table },
        Copy::Update => Format::Update {
            table,
            primary_keys: keys,
        },
        Copy::Json(camel) => Format::Json { camel },
        Copy::Comments => Format::Comments,
        Copy::Tsv(headers, data) => Format::Tsv { headers, data },
        Copy::Plain => Format::Tsv {
            headers: false,
            data: true,
        },
    }
}
