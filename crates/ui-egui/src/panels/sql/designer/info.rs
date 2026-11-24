//! Source information-table geometry, single selection, resize and sorting.
use super::*;
mod form;
mod preview;
pub(super) fn render(
    ui: &mut Ui,
    draft: &mut Draft,
    icons: &mut crate::icons::Icons,
    writable: bool,
) -> Option<Intent> {
    let (index, label) = match draft.page {
        Page::Indexes => (0, "索引"),
        Page::ForeignKeys => (1, "外键"),
        _ => (2, "触发器"),
    };
    use tomato_core::table_design::alter::InfoPage;
    let page = if index == 1 {
        InfoPage::ForeignKeys
    } else {
        InfoPage::Triggers
    };
    let empty = tomato_core::table_design::alter::InfoTable::default();
    let table = draft.saved.as_ref().map_or(&empty, |saved| {
        if index == 1 {
            &saved.foreign_keys
        } else {
            &saved.triggers
        }
    });
    let state = &mut draft.info[index];
    let available = writable
        && !matches!(draft.dialect, tomato_core::table_design::Dialect::Hive)
        && (draft.saved.is_none() || draft.info_loaded[index]);
    let mut changed = false;
    let bounds = ui.max_rect();
    let toolbar = Rect::from_min_size(bounds.min, vec2(bounds.width(), 35.));
    ui.painter()
        .rect_filled(toolbar, 0, Color32::from_gray(248));
    ui.painter().rect_filled(
        Rect::from_min_max(pos2(toolbar.left(), toolbar.bottom() - 1.), toolbar.max),
        0,
        Color32::from_gray(221),
    );
    let mut x = bounds.left() + 8.;
    let mut refresh = false;
    for (which, (text, asset, width)) in [
        (
            format!("添加{label}"),
            "col_add.png",
            if index == 2 { 98. } else { 86. },
        ),
        ("删除".into(), "col_del.png", 62.),
        ("刷新".into(), "refresh.png", 61.),
    ]
    .into_iter()
    .enumerate()
    {
        let rect = Rect::from_min_size(pos2(x, bounds.top() + 4.), vec2(width, 26.));
        let enabled = if which == 2 {
            !state.changed(&table.rows) && draft.saved.is_some()
        } else {
            available
                && if which == 0 {
                    state.rows.as_ref().map_or(table.rows.len(), Vec::len) < 1024
                } else {
                    state.selected.is_some()
                }
        };
        let response = super::button(ui, icons, rect, &text, asset, enabled);
        if response.clicked() {
            match which {
                0 => changed |= state.add(&table.rows, page, &draft.request.table.schema),
                1 => changed |= state.delete(&table.rows),
                _ => refresh = true,
            }
        }
        x += width + 2.;
    }
    let mut resized = None;
    let form_height = if state.form.is_some() {
        (bounds.height() * 0.60).min(340.)
    } else {
        0.
    };
    let content = Rect::from_min_max(toolbar.left_bottom(), bounds.max - vec2(0., form_height));
    let rows = state.rows.as_deref().unwrap_or(&table.rows);
    let headers = tomato_core::table_design::auxiliary::headers(page);
    let mut sort_column = None;
    if state.widths.is_empty() {
        state.widths = match index {
            0 => vec![180., 200., 100., 100., 60., 200.],
            1 => vec![160., 150., 120., 150., 150., 100., 100.],
            _ => vec![180., 100., 100., 500.],
        };
    }
    if state.order.len() != rows.len() {
        state.order.clear();
        state.order.extend(0..rows.len());
        state.selected = None;
    }
    ui.painter()
        .rect_filled(content, 0, Color32::from_gray(244));
    if !rows.is_empty() {
        ui.scope_builder(egui::UiBuilder::new().max_rect(content.shrink(1.)), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(content));
            ui.spacing_mut().scroll.floating = false;
            egui::ScrollArea::horizontal()
                .id_salt(("info-horizontal", index))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let width = state.widths.iter().sum::<f32>();
                    ui.set_min_width(width);
                    let (header, _) = ui.allocate_exact_size(vec2(width, 30.), Sense::hover());
                    let mut x = header.left();
                    let mut sort = None;
                    for (column, title) in headers.iter().enumerate() {
                        let rect = Rect::from_min_size(
                            pos2(x, header.top()),
                            vec2(state.widths[column], 30.),
                        );
                        super::grid::cell(ui, rect, Color32::from_gray(245), true, false);
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            title,
                            crate::native_dialog::font(ui.ctx(), 12.),
                            Color32::BLACK,
                        );
                        if ui
                            .interact(
                                rect.shrink2(vec2(3., 0.)),
                                ui.id().with(("info-header", index, column)),
                                Sense::click(),
                            )
                            .clicked()
                        {
                            sort = Some(column);
                        }
                        let resize = ui.interact(
                            Rect::from_center_size(rect.right_center(), vec2(5., 30.)),
                            ui.id().with(("info-resize", index, column)),
                            Sense::drag(),
                        );
                        if resize.dragged() {
                            resized = Some((
                                column,
                                (state.widths[column] + ui.input(|i| i.pointer.delta().x)).max(50.),
                            ));
                        }
                        x += state.widths[column];
                    }
                    if let Some(column) = sort {
                        sort_column = Some(column);
                    }
                    egui::ScrollArea::vertical()
                        .id_salt(("info-vertical", index))
                        .auto_shrink([false, false])
                        .show_rows(ui, 30., rows.len(), |ui, range| {
                            for visual in range {
                                let row = state.order[visual];
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(width, 30.), Sense::hover());
                                let selected = state.selected == Some(row);
                                let response = ui.interact(
                                    rect,
                                    ui.id().with(("info-row", index, row)),
                                    Sense::click(),
                                );
                                if response.clicked() {
                                    state.selected = Some(row);
                                }
                                if response.double_clicked() {
                                    state.form = Some(row);
                                    state.focus_form = true;
                                }
                                let mut x = rect.left();
                                for (column, cell) in rows[row].iter().enumerate() {
                                    let cell_rect = Rect::from_min_size(
                                        pos2(x, rect.top()),
                                        vec2(state.widths[column], 30.),
                                    );
                                    x += state.widths[column];
                                    super::grid::cell(
                                        ui,
                                        cell_rect,
                                        if selected {
                                            Color32::from_rgb(53, 146, 203)
                                        } else {
                                            Color32::WHITE
                                        },
                                        false,
                                        false,
                                    );
                                    ui.painter()
                                        .with_clip_rect(cell_rect.shrink2(vec2(6., 0.)))
                                        .text(
                                            cell_rect.left_center() + vec2(6., 0.),
                                            Align2::LEFT_CENTER,
                                            preview::single_line(cell).as_ref(),
                                            crate::native_dialog::font(ui.ctx(), 12.),
                                            if selected {
                                                Color32::WHITE
                                            } else {
                                                Color32::BLACK
                                            },
                                        );
                                }
                            }
                        });
                });
        });
    } else {
        ui.painter().rect_filled(
            Rect::from_min_size(content.min, vec2(content.width(), 26.)),
            0,
            Color32::WHITE,
        );
    }
    if rows.is_empty() {
        ui.painter().text(
            content.center() + vec2(0., 15.),
            Align2::CENTER_CENTER,
            "暂无数据",
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::BLACK,
        );
    }
    if let Some(column) = sort_column {
        let staged = state.rows.take();
        state.sort(staged.as_deref().unwrap_or(&table.rows), column);
        state.rows = staged;
    }
    let mut form_ids = Vec::new();
    if form_height > 0. {
        let rect = Rect::from_min_max(
            pos2(bounds.left() + 8., content.bottom() + 6.),
            bounds.max - vec2(8., 4.),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(("aux-form-region", index))
                .max_rect(rect),
            |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(rect));
                let result = form::show(ui, state, &table.rows, index, draft.dialect, available);
                changed |= result.0;
                form_ids = result.1;
            },
        );
    }
    if let Some((column, width)) = resized {
        state.widths[column] = width;
        ui.ctx().request_repaint();
    }
    for id in form_ids {
        draft.own_text(id);
    }
    if changed {
        draft.changed();
        draft.status = format!("{label}已修改（未保存）");
    }
    if refresh {
        use tomato_core::table_design::alter::InfoPage;
        Some(Intent::RefreshDesignInfo(match index {
            0 => InfoPage::Indexes,
            1 => InfoPage::ForeignKeys,
            _ => InfoPage::Triggers,
        }))
    } else {
        None
    }
}
