use super::*;
use egui::{Color32, Rect, Sense, Stroke, pos2};
use tomato_core::development::runtime::{self, EntryRole};
pub(super) enum Action {
    Add(EntryRole),
    Edit(usize),
    Remove(EntryRole),
}
fn count(draft: &Draft, role: EntryRole) -> usize {
    if role.is_build() {
        draft.config.build_tools.len()
    } else {
        draft.config.runtimes.len()
    }
}
fn height(draft: &Draft, role: EntryRole, index: usize) -> f32 {
    if role.is_build()
        && !draft.config.build_tools[index]
            .settings_path
            .trim()
            .is_empty()
    {
        59.
    } else {
        42.
    }
}
pub(super) fn show(ui: &mut Ui, draft: &mut Draft, role: EntryRole) -> Option<Action> {
    let area = ui.max_rect();
    let origin = area.min;
    let list = Rect::from_min_size(origin + vec2(24., 53.), vec2(area.width() - 48., 350.));
    ui.painter().rect_filled(list, 0, Color32::WHITE);
    ui.painter().rect_stroke(
        list,
        0,
        Stroke::new(1., Color32::from_gray(200)),
        egui::StrokeKind::Inside,
    );
    let id = ui.id().with(("sdk-list", role));
    let focused = ui.memory(|m| m.has_focus(id));
    let mut selected = if role.is_build() {
        draft.selected_tool
    } else {
        draft.selected_runtime
    };
    let n = count(draft, role);
    let mut reveal = false;
    if focused && n > 0 {
        let up = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
        let down = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
        let home = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Home));
        let end = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::End));
        if up || down || home || end {
            selected = Some(if home {
                0
            } else if end {
                n - 1
            } else if up {
                selected.unwrap_or(0).saturating_sub(1)
            } else {
                selected.map_or(0, |i| (i + 1).min(n - 1))
            });
            reveal = true;
        }
    }
    let mut action = None;
    let inner = list.shrink(1.);
    let list_hit = ui.interact(inner, id, Sense::click());
    if list_hit.clicked() {
        list_hit.request_focus();
    }
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(inner));
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        ui.spacing_mut().scroll = egui::style::ScrollStyle::solid();
        ui.spacing_mut().scroll.bar_width = 12.;
        egui::ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false])
            .show_viewport(ui, |ui, viewport| {
                let top = ui.cursor().min;
                let width = ui.available_width();
                let total = (0..n).map(|i| height(draft, role, i)).sum::<f32>();
                ui.set_min_height(total.max(inner.height()));
                let mut y = 0.;
                for index in 0..n {
                    let h = height(draft, role, index);
                    let row = Rect::from_min_size(top + vec2(0., y), vec2(width, h));
                    if reveal && selected == Some(index) {
                        ui.scroll_to_rect(row, None);
                    }
                    if y + h >= viewport.top() && y <= viewport.bottom() {
                        let hit = ui.interact(row, id.with(index), Sense::click());
                        let active = selected == Some(index);
                        ui.painter().rect_filled(
                            row,
                            0,
                            if active {
                                if focused {
                                    Color32::from_rgb(0, 150, 201)
                                } else {
                                    Color32::from_gray(211)
                                }
                            } else {
                                Color32::from_gray(if index % 2 == 0 { 255 } else { 249 })
                            },
                        );
                        let (kind, name, version, path, settings) = if role.is_build() {
                            let v = &draft.config.build_tools[index];
                            (
                                &v.kind,
                                &v.name,
                                &v.version,
                                &v.home_path,
                                v.settings_path.as_str(),
                            )
                        } else {
                            let v = &draft.config.runtimes[index];
                            (&v.kind, &v.name, &v.version, &v.path, "")
                        };
                        let bold = egui::FontId::new(13., egui::FontFamily::Name("md-bold".into()));
                        let tag_font =
                            egui::FontId::new(10., egui::FontFamily::Name("md-bold".into()));
                        let tag_width = ui
                            .painter()
                            .layout_no_wrap(kind.clone(), tag_font.clone(), Color32::from_gray(51))
                            .size()
                            .x
                            .ceil()
                            + 14.;
                        let tag = Rect::from_min_size(row.min + vec2(7., 4.), vec2(tag_width, 17.));
                        ui.painter().rect_filled(
                            tag,
                            4,
                            if role.is_build() {
                                Color32::from_rgb(255, 243, 224)
                            } else {
                                Color32::from_rgb(232, 244, 255)
                            },
                        );
                        ui.painter().text(
                            tag.center(),
                            egui::Align2::CENTER_CENTER,
                            kind,
                            tag_font,
                            if role.is_build() {
                                Color32::from_rgb(239, 108, 0)
                            } else {
                                Color32::from_rgb(25, 118, 210)
                            },
                        );
                        ui.painter().with_clip_rect(row.intersect(inner)).text(
                            pos2(tag.right() + 8., row.top() + 4.),
                            egui::Align2::LEFT_TOP,
                            format!("{name}  {}", runtime::known_version(version)),
                            bold,
                            if active && focused {
                                Color32::WHITE
                            } else {
                                Color32::from_gray(51)
                            },
                        );
                        ui.painter().with_clip_rect(row.intersect(inner)).text(
                            row.min + vec2(7., 25.),
                            egui::Align2::LEFT_TOP,
                            path,
                            controls::font(ui, 11.),
                            Color32::from_gray(if role.is_build() { 119 } else { 136 }),
                        );
                        if !settings.trim().is_empty() {
                            ui.painter().with_clip_rect(row.intersect(inner)).text(
                                row.min + vec2(7., 42.),
                                egui::Align2::LEFT_TOP,
                                format!("settings.xml: {settings}"),
                                controls::font(ui, 10.),
                                Color32::from_gray(153),
                            );
                        }
                        if hit.clicked() || hit.secondary_clicked() {
                            selected = Some(index);
                            ui.memory_mut(|m| m.request_focus(id));
                        }
                        if role.is_build() && hit.double_clicked() {
                            action = Some(Action::Edit(index));
                        }
                    }
                    y += h;
                }
                let first = ((viewport.top() - total).max(0.) / 21.).floor() as usize;
                let last = ((viewport.bottom() - total).max(0.) / 21.).ceil() as usize;
                for index in first..last {
                    let row = Rect::from_min_size(
                        top + vec2(0., total + index as f32 * 21.),
                        vec2(width, 21.),
                    );
                    ui.painter().rect_filled(
                        row,
                        0,
                        Color32::from_gray(if (n + index).is_multiple_of(2) {
                            255
                        } else {
                            249
                        }),
                    );
                }
            });
    });
    if role.is_build() {
        draft.selected_tool = selected;
    } else {
        draft.selected_runtime = selected;
    }
    let y = origin.y + 413.;
    let right = area.right() - 24.;
    if controls::button(
        ui,
        Rect::from_min_size(pos2(right - 40., y), vec2(40., 24.)),
        "删除",
        false,
        selected.is_some(),
    )
    .clicked()
    {
        action = Some(Action::Remove(role));
    }
    if role.is_build() {
        if controls::button(
            ui,
            Rect::from_min_size(pos2(right - 88., y), vec2(40., 24.)),
            "修改",
            false,
            selected.is_some(),
        )
        .clicked()
            && let Some(index) = selected
        {
            action = Some(Action::Edit(index));
        }
        if controls::button(
            ui,
            Rect::from_min_size(pos2(right - 184., y), vec2(88., 24.)),
            "添加构建工具",
            false,
            true,
        )
        .clicked()
        {
            action = Some(Action::Add(role));
        }
    } else if controls::button(
        ui,
        Rect::from_min_size(pos2(right - 116., y), vec2(68., 24.)),
        "添加 SDK",
        false,
        true,
    )
    .clicked()
    {
        action = Some(Action::Add(role));
    }
    action
}
