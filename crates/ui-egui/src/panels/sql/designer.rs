//! TableStructureView toolbar, seven pages and editor/controller boundary.
use super::*;
use crate::tabs::table_design::{Action, Cell, Draft, Page};
use egui::{Align2, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
mod dialogs;
mod fields;
mod grid;
mod indexes;
mod info;
mod pages;
mod preview;
mod selection;
mod type_picker;
fn button(
    ui: &mut Ui,
    icons: &mut crate::icons::Icons,
    rect: Rect,
    label: &str,
    asset: &str,
    enabled: bool,
) -> egui::Response {
    let enabled = enabled && ui.is_enabled();
    let response = ui.interact(
        rect,
        ui.id().with(("designer-action", label)),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let pressed = enabled && response.is_pointer_button_down_on();
    let hover = enabled && response.hovered();
    ui.painter().rect_filled(
        rect,
        0,
        if pressed {
            Color32::from_rgb(192, 220, 243)
        } else if hover {
            Color32::from_rgb(216, 230, 242)
        } else {
            Color32::from_gray(248)
        },
    );
    if hover || pressed {
        ui.painter().rect_stroke(
            rect,
            0,
            Stroke::new(
                1.,
                if pressed {
                    Color32::from_rgb(144, 200, 246)
                } else {
                    Color32::from_gray(204)
                },
            ),
            StrokeKind::Inside,
        );
    }
    if !asset.is_empty()
        && let Some(texture) = icons.get(ui.ctx(), &format!("images/connect/{asset}"))
    {
        ui.painter().image(
            texture,
            Rect::from_min_size(rect.min + vec2(9., 5.), vec2(16., 16.)),
            Rect::from_min_max(egui::Pos2::ZERO, pos2(1., 1.)),
            Color32::WHITE.gamma_multiply(if enabled { 1. } else { 0.4 }),
        );
    }
    ui.painter().text(
        pos2(
            rect.left() + if asset.is_empty() { 9. } else { 29. },
            rect.center().y,
        ),
        Align2::LEFT_CENTER,
        label,
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::BLACK.gamma_multiply(if enabled { 1. } else { 0.4 }),
    );
    response
}
pub(super) fn render(
    ui: &mut Ui,
    tab: &mut Tab,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        return None;
    };
    let draft = workbench.designer.as_mut()?;
    let bounds = ui.available_rect_before_wrap();
    let mut intent = None;
    ui.spacing_mut().item_spacing = vec2(0., 0.);
    ui.painter().rect_filled(bounds, 0, Color32::WHITE);
    let blocked = draft.name.is_some() || draft.notice.is_some();
    let writable =
        !draft.busy && !tab.busy && !blocked && !draft.saved.as_ref().is_some_and(|s| s.view);
    ui.painter().rect_filled(
        Rect::from_min_size(bounds.min, vec2(bounds.width(), 35.)),
        0,
        Color32::from_gray(248),
    );
    let mut x = bounds.left() + 8.;
    for (index, (label, asset, width)) in [
        ("保存", "save.png", 62.),
        ("添加字段", "col_add.png", 86.),
        ("插入字段", "col_jump.png", 86.),
        ("删除", "col_del.png", 62.),
        ("主键", "primary_key.png", 62.),
        ("上移", "up.png", 62.),
        ("下移", "down.png", 62.),
        ("刷新", "refresh.png", 61.),
    ]
    .into_iter()
    .enumerate()
    {
        if index == 7 {
            x += 16.;
            ui.painter().rect_filled(
                Rect::from_min_size(pos2(x - 10., bounds.top() + 4.), vec2(1., 26.)),
                0,
                Color32::from_gray(204),
            );
        }
        let rect = Rect::from_min_size(pos2(x, bounds.top() + 4.), vec2(width, 26.));
        if button(
            ui,
            icons,
            rect,
            label,
            asset,
            if index == 7 {
                !draft.busy && !blocked
            } else {
                writable
            },
        )
        .clicked()
        {
            match index {
                0 => intent = Some(Intent::SaveDesign),
                7 => intent = Some(Intent::RefreshDesign),
                _ => draft.perform(
                    ui.ctx(),
                    [
                        Action::Add,
                        Action::Insert,
                        Action::Delete,
                        Action::PrimaryKey,
                        Action::Up,
                        Action::Down,
                    ][index - 1],
                ),
            }
        }
        x += width + 2.;
    }
    let pages = [
        (Page::Fields, "字段"),
        (Page::Indexes, "索引"),
        (Page::ForeignKeys, "外键"),
        (Page::Triggers, "触发器"),
        (Page::Options, "选项"),
        (Page::Comment, "注释"),
        (Page::Sql, "SQL预览"),
    ];
    ui.painter().rect_filled(
        Rect::from_min_size(
            pos2(bounds.left(), bounds.top() + 34.),
            vec2(bounds.width(), 1.),
        ),
        0,
        Color32::from_gray(221),
    );
    let strip = Rect::from_min_size(
        pos2(bounds.left(), bounds.top() + 35.),
        vec2(bounds.width(), 25.),
    );
    let mut page = draft.page;
    ui.add_enabled_ui(!blocked, |ui| {
        crate::native_tabs::show_at(
            ui,
            "designer-pages",
            strip,
            &mut page,
            crate::native_tabs::Style::Compact,
            pages,
        );
    });
    if page != draft.page {
        draft.switch_page(ui.ctx(), page);
    }
    let content = Rect::from_min_max(
        pos2(bounds.left(), bounds.top() + 60.),
        pos2(bounds.right(), bounds.bottom() - 30.),
    );
    ui.scope_builder(egui::UiBuilder::new().max_rect(content), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(content));
        if blocked || draft.busy || tab.busy {
            ui.disable();
        }
        if draft.page == Page::Fields {
            fields::render(ui, draft, icons, writable);
        } else if draft.page == Page::Indexes {
            if let Some(action) = indexes::render(ui, draft, icons, writable) {
                intent = Some(action);
            }
        } else if matches!(
            draft.page,
            Page::Indexes | Page::ForeignKeys | Page::Triggers
        ) {
            if let Some(action) = info::render(ui, draft, icons, writable) {
                intent = Some(action);
            }
        } else if let Some(action) = pages::render(ui, draft, writable) {
            intent = Some(action);
        }
    });
    if draft.busy || tab.busy {
        ui.put(
            Rect::from_center_size(content.center(), vec2(40., 40.)),
            egui::Spinner::new()
                .size(40.)
                .color(Color32::from_rgb(0, 150, 201)),
        );
    }
    let footer = Rect::from_min_max(pos2(bounds.left(), bounds.bottom() - 30.), bounds.max);
    ui.painter().rect_filled(footer, 0, Color32::from_gray(245));
    ui.painter().line_segment(
        [footer.left_top(), footer.right_top()],
        Stroke::new(1., Color32::from_gray(221)),
    );
    let status_id = ui.id().with("designer-status");
    draft.own_text(status_id);
    let mut status = draft.status.as_str();
    ui.put(
        footer.shrink2(vec2(12., 4.)),
        TextEdit::singleline(&mut status)
            .id(status_id)
            .frame(egui::Frame::NONE)
            .font(crate::native_dialog::font(ui.ctx(), 12.)),
    );
    if !blocked && ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S)) {
        intent = Some(Intent::SaveDesign);
    }
    if let Some(name) = dialogs::render(ui.ctx(), draft, icons) {
        intent = Some(Intent::CreateDesign(name));
    }
    if !blocked && !draft.busy && !tab.busy && draft.saved.is_some() {
        use tomato_core::table_design::alter::InfoPage;
        let page = match draft.page {
            Page::Indexes => Some((0, InfoPage::Indexes)),
            Page::ForeignKeys => Some((1, InfoPage::ForeignKeys)),
            Page::Triggers => Some((2, InfoPage::Triggers)),
            _ => None,
        };
        if let Some((index, page)) = page
            && !draft.info_loaded[index]
            && intent.is_none()
        {
            draft.info_loaded[index] = true;
            intent = Some(Intent::RefreshDesignInfo(page));
        }
        if draft.page == Page::Sql
            && draft
                .saved
                .as_ref()
                .is_some_and(|saved| saved.view || draft.preview_save_as)
            && matches!(
                draft.sql_definition,
                crate::tabs::table_design::SqlDefinition::Unloaded
            )
            && intent.is_none()
        {
            intent = Some(Intent::LoadDesignSql);
        }
    }
    if !blocked
        && ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Copy))
        })
    {
        let selection = ui
            .memory(|memory| memory.focused())
            .and_then(|id| egui::widgets::text_edit::TextEditState::load(ui.ctx(), id))
            .and_then(|state| state.cursor.char_range());
        if selection.is_some_and(|range| !range.is_empty()) {
            draft.copied_fields = None;
        } else if draft.edit.is_none() {
            match draft.copy_fields() {
                Ok(Some(text)) => ui.ctx().copy_text(text.to_owned()),
                Err(error) => draft.status = error.to_string(),
                _ => {}
            }
        }
    }
    draft.trim_text(ui.ctx());
    ui.allocate_rect(bounds, Sense::hover());
    intent
}
