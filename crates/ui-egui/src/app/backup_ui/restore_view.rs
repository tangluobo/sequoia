//! Original RestoreDialog geometry, driven by the same bounded actor progress bridge.
use super::{
    draft::Draft,
    view::{Action, button, label},
};
use egui::{Align2, Color32, Rect, Ui};
pub(super) fn render(
    ui: &mut Ui,
    draft: &mut Draft,
    directory: Option<&crate::app::backup_tree::Directory>,
) -> Option<Action> {
    let origin = ui.max_rect().min;
    let size = ui.max_rect().size();
    let at = |x, y, w, h| Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h));
    let mut action = None;
    ui.style_mut().override_font_id = Some(crate::native_dialog::font(ui.ctx(), 13.));
    ui.painter().rect_filled(ui.max_rect(), 0., Color32::WHITE);
    ui.painter()
        .rect_filled(at(0., 0., size.x, 37.), 0., Color32::from_gray(236));
    for (index, (text, x, w)) in [("常规", 0., 49.), ("信息日志", 49., 73.)]
        .into_iter()
        .enumerate()
    {
        let rect = at(x, 0., w, 37.);
        let response = ui.interact(
            rect,
            ui.id().with(("restore-tab", index)),
            egui::Sense::click(),
        );
        ui.painter().rect_filled(
            rect,
            0.,
            Color32::from_gray(if draft.tab == index { 255 } else { 226 }),
        );
        ui.painter().line_segment(
            [rect.right_top(), rect.right_bottom()],
            egui::Stroke::new(1., Color32::from_gray(187)),
        );
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            text,
            crate::native_dialog::font(ui.ctx(), 12.),
            Color32::BLACK,
        );
        if response.clicked() {
            draft.tab = index;
        }
    }
    let bottom = size.y - 49.;
    if draft.tab == 0 {
        for (index, (caption, value)) in [
            ("服务器:", draft.connection.name.as_str()),
            ("数据库:", draft.scope.database.as_str()),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 37. + index as f32 * 26.;
            label(ui, at(0., y, 80., 18.), caption, 13., Color32::BLACK);
            label(
                ui,
                at(90., y, size.x - 90., 18.),
                value,
                13.,
                Color32::BLACK,
            );
        }
        label(ui, at(0., 89., 80., 18.), "备份文件:", 13., Color32::BLACK);
        if draft.choose_restore_source {
            action = super::restore_source::render(ui, at(90., 86., 370., 24.), draft, directory)
                .map(Action::RestoreSource);
        } else {
            label(
                ui,
                at(90., 89., size.x - 90., 18.),
                draft.restore.as_deref().unwrap_or_default(),
                13.,
                Color32::BLACK,
            );
        }
        ui.painter().line_segment(
            [
                origin + egui::vec2(0., 127.5),
                origin + egui::vec2(size.x, 127.5),
            ],
            egui::Stroke::new(1., Color32::from_gray(215)),
        );
        label(ui, at(0., 150., 80., 18.), "进度:", 13., Color32::BLACK);
        let (total, objects, records) = draft
            .control
            .read(|progress| (progress.total, progress.objects, progress.records))
            .unwrap_or_default();
        let progress = if draft.complete.is_some() {
            1.
        } else if total == 0 {
            0.
        } else {
            (objects as f32 / total as f32).min(1.)
        };
        let bar = at(0., 180., 400., 18.);
        crate::native_dialog::gradient(ui, bar, Color32::from_gray(255), Color32::from_gray(235));
        ui.painter().rect_stroke(
            bar,
            2.,
            egui::Stroke::new(1., Color32::from_gray(181)),
            egui::StrokeKind::Inside,
        );
        if progress > 0. {
            ui.painter().rect_filled(
                Rect::from_min_size(
                    bar.min + egui::vec2(1., 1.),
                    egui::vec2(398. * progress, 16.),
                ),
                1.,
                Color32::from_rgb(7, 193, 96),
            );
        }
        let objects = if draft.started.is_some() {
            objects.to_string()
        } else {
            "-".into()
        };
        let records = if draft.started.is_some() {
            records.to_string()
        } else {
            "-".into()
        };
        let elapsed = if draft.running {
            "进行中...".into()
        } else if draft.error.is_some() {
            "失败".into()
        } else if let Some(ms) = draft.elapsed_ms {
            format!("{:.2} 秒", ms as f64 / 1000.)
        } else {
            "-".into()
        };
        for (index, (caption, value)) in [
            ("对象:", "-"),
            ("已处理对象:", objects.as_str()),
            ("已处理记录:", records.as_str()),
            ("时间:", elapsed.as_str()),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 210. + index as f32 * 23.;
            label(ui, at(0., y, 80., 18.), caption, 13., Color32::BLACK);
            label(
                ui,
                at(90., y, size.x - 90., 18.),
                value,
                13.,
                Color32::BLACK,
            );
        }
        label(
            ui,
            at(0., 309., size.x, 15.),
            "注意：还原操作将覆盖目标数据库中已存在的同名表/视图/函数/事件。",
            11.,
            Color32::from_rgb(204, 0, 0),
        );
    } else {
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(at(0., 37., size.x, (bottom - 37.).max(18.))),
            |ui| {
                super::log_view::render(ui, draft);
            },
        );
    }
    ui.painter()
        .rect_filled(at(0., bottom, size.x, 49.), 0., Color32::from_gray(240));
    ui.painter().line_segment(
        [
            origin + egui::vec2(0., bottom + 0.5),
            origin + egui::vec2(size.x, bottom + 0.5),
        ],
        egui::Stroke::new(1., Color32::from_gray(221)),
    );
    let start = ui
        .add_enabled_ui(!draft.running && draft.restore.is_some(), |ui| {
            button(
                ui,
                at(size.x - 200., size.y - 38., 100., 28.),
                "开始还原",
                true,
            )
        })
        .inner;
    if start.clicked() {
        action = Some(Action::Start);
    }
    let close = button(ui, at(size.x - 90., size.y - 38., 80., 28.), "关闭", false);
    if close.clicked()
        || ui.input(|input| {
            input.viewport().close_requested() || input.key_pressed(egui::Key::Escape)
        })
    {
        action = Some(Action::Close);
    }
    if super::preview::active() {
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("tomato-backup-button-bounds"),
                [
                    start.rect.translate(-origin.to_vec2()),
                    close.rect.translate(-origin.to_vec2()),
                ],
            )
        });
    }
    action
}
