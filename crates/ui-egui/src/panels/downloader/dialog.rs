use super::*;
use crate::tabs::downloader::{Dialog, Kind, State};
use tomato_services::downloader::Settings;

pub(super) fn show(ctx: &egui::Context, state: &mut State) {
    let Some(dialog) = &mut state.dialog else {
        return;
    };
    let settings = dialog.kind == Kind::Settings;
    let viewport = egui::ViewportId::from_hash_of(if settings {
        "download-settings"
    } else {
        "download-add"
    });
    let size = egui::vec2(590., if settings { 243. } else { 218. });
    let mut builder = egui::ViewportBuilder::default()
        .with_title(if settings {
            "下载器设置"
        } else {
            "添加下载"
        })
        .with_inner_size(size)
        .with_min_inner_size(size)
        .with_resizable(true)
        .with_minimize_button(false)
        .with_maximize_button(false);
    if dialog.focus
        && let Some(owner) = ctx.input(|input| input.viewport().inner_rect)
    {
        builder = builder.with_position(owner.center() - size * 0.5);
    }
    let mut finished = false;
    let mut action = None;
    let mut defaults = None;
    let capture = std::env::var_os("TOMATO_UI_CAPTURE").is_some()
        && matches!(
            std::env::var("TOMATO_UI_PREVIEW").as_deref(),
            Ok("downloader" | "downloader_saved")
        );
    if capture {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), viewport));
    }
    ctx.show_viewport_immediate(viewport, builder, |ui, _| {
        if capture
            && let Some(image) = ui.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot {
                        viewport_id, image, ..
                    } = event
                        && *viewport_id == viewport
                    {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            })
        {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-image"), image));
        }
        let rect = ui.max_rect();
        let width = rect.width();
        let height = rect.height();
        let at = |x: f32, y: f32, w: f32, h: f32| {
            egui::Rect::from_min_size(rect.min + egui::vec2(x, y), egui::vec2(w, h))
        };
        ui.painter().rect_filled(rect, 0, Color32::from_gray(244));
        crate::native_dialog::gradient(
            ui,
            at(0., 0., width, 38.),
            Color32::from_gray(250),
            Color32::from_gray(244),
        );
        ui.painter().hline(
            rect.x_range(),
            rect.top() + 38.,
            egui::Stroke::new(1., Color32::from_gray(200)),
        );
        ui.painter().text(
            rect.min + egui::vec2(10., 10.),
            egui::Align2::LEFT_TOP,
            if settings {
                "设置下载器默认值"
            } else {
                "添加新的下载任务"
            },
            crate::native_dialog::font(ui.ctx(), 14.),
            Color32::BLACK,
        );
        let left = if settings { 84. } else { 60. };
        let right = width - 87.;
        if !settings {
            caption(ui, at(2., 48., 48., 23.), "下载地址");
            let response = edit(
                ui,
                at(left, 48., right - left, 23.),
                &mut dialog.url,
                "HTTP/HTTPS、magnet、ED2K 或 .torrent 文件",
                8192,
            );
            if dialog.focus {
                response.request_focus();
            }
            if button(ui, at(width - 77., 47., 75., 26.), "选择种子...", false).clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("种子文件", &["torrent"])
                    .pick_file()
            {
                dialog.url = path.display().to_string();
            }
        }
        let y = if settings { 48. } else { 85. };
        caption(
            ui,
            at(2., y, left - 12., 23.),
            if settings {
                "默认下载目录"
            } else {
                "保存目录"
            },
        );
        let input_right = if settings { width - 63. } else { right };
        edit(
            ui,
            at(left, y, input_right - left, 23.),
            &mut dialog.directory,
            "保存目录",
            32768,
        );
        if button(
            ui,
            at(input_right + 10., y - 1., 51., 26.),
            "浏览...",
            false,
        )
        .clicked()
            && let Some(path) = rfd::FileDialog::new().pick_folder()
        {
            dialog.directory = path.display().to_string();
        }
        let y = if settings { 84. } else { 121. };
        caption(
            ui,
            at(2., y, left - 12., 23.),
            if settings {
                "默认任务线程"
            } else {
                "并行数"
            },
        );
        spinner(
            ui,
            at(left, y, 90., 23.),
            &mut dialog.threads,
            32,
            "download-threads",
        );
        if settings {
            caption(ui, at(2., 119., left - 12., 23.), "同时下载任务");
            spinner(
                ui,
                at(left, 119., 90., 23.),
                &mut dialog.concurrent,
                8,
                "download-concurrent",
            );
            ui.painter().text(
                rect.min + egui::vec2(left, 154.),
                egui::Align2::LEFT_TOP,
                "同时下载任务数修改后会立即应用，正在运行的任务不会被中断。",
                crate::native_dialog::font(ui.ctx(), 11.),
                Color32::from_gray(119),
            );
        }
        if let Some(error) = &dialog.error {
            ui.painter().text(
                rect.min + egui::vec2(left, if settings { 177. } else { 153. }),
                egui::Align2::LEFT_TOP,
                error,
                crate::native_dialog::font(ui.ctx(), 11.),
                Color32::from_rgb(211, 47, 47),
            );
        }
        let submit = button(
            ui,
            at(width - 170., height - 33., 75., 23.),
            if settings { "保存" } else { "添加下载" },
            true,
        )
        .clicked();
        let cancel = button(ui, at(width - 85., height - 33., 75., 23.), "取消", false).clicked()
            || ui.input(|input| {
                input.key_pressed(egui::Key::Escape) || input.viewport().close_requested()
            });
        if submit || ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            match validated(dialog) {
                Ok(value) => {
                    if settings {
                        defaults = Some(value.clone());
                        action = Some(Action::Configure(value));
                    } else {
                        action = Some(Action::Add {
                            url: dialog.url.trim().to_owned(),
                            directory: PathBuf::from(&value.directory),
                            threads: value.threads,
                        });
                    }
                    finished = true;
                }
                Err(error) => dialog.error = Some(error.to_string()),
            }
        }
        if cancel {
            finished = true;
            action = None;
            defaults = None;
        }
        dialog.focus = false;
    });
    if let Some(defaults) = defaults {
        state.defaults = defaults;
    }
    if let Some(action) = action {
        state.send(action);
    }
    if finished {
        state.dialog = None;
        ctx.send_viewport_cmd_to(viewport, egui::ViewportCommand::Close);
        ctx.request_repaint();
    }
}
fn validated(dialog: &Dialog) -> anyhow::Result<Settings> {
    anyhow::ensure!(!dialog.directory.trim().is_empty(), "请选择保存目录");
    let threads = dialog
        .threads
        .parse::<usize>()
        .ok()
        .filter(|v| (1..=32).contains(v))
        .ok_or_else(|| anyhow::anyhow!("并行数必须为 1–32"))?;
    let concurrent = dialog
        .concurrent
        .parse::<usize>()
        .ok()
        .filter(|v| (1..=8).contains(v))
        .ok_or_else(|| anyhow::anyhow!("同时下载任务必须为 1–8"))?;
    if dialog.kind == Kind::Add {
        tomato_services::downloader::validate_source(&dialog.url)?;
    }
    Ok(Settings {
        directory: dialog.directory.trim().into(),
        threads,
        concurrent,
    })
}
fn caption(ui: &Ui, rect: egui::Rect, text: &str) {
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        crate::native_dialog::font(ui.ctx(), 12.),
        Color32::BLACK,
    );
}
fn edit(
    ui: &mut Ui,
    rect: egui::Rect,
    value: &mut String,
    hint: &str,
    limit: usize,
) -> egui::Response {
    ui.place(
        rect,
        TextEdit::singleline(value)
            .font(crate::native_dialog::font(ui.ctx(), 12.))
            .hint_text(hint)
            .char_limit(limit)
            .margin(egui::Margin::symmetric(8, 3)),
    )
}
fn spinner(ui: &mut Ui, rect: egui::Rect, value: &mut String, max: usize, salt: &str) {
    let mut field = rect;
    field.max.x -= 22.;
    edit(ui, field, value, "", 3);
    for (index, delta) in [(0, 1isize), (1, -1)] {
        let button = egui::Rect::from_min_size(
            rect.right_top() + egui::vec2(-22., index as f32 * 11.5),
            egui::vec2(22., 11.5),
        );
        crate::native_dialog::gradient(
            ui,
            button,
            Color32::from_gray(248),
            Color32::from_gray(224),
        );
        ui.painter().rect_stroke(
            button,
            0,
            egui::Stroke::new(1., Color32::from_gray(177)),
            egui::StrokeKind::Inside,
        );
        let response = ui.interact(button, ui.id().with((salt, index)), egui::Sense::click());
        let center = button.center();
        let sign = if index == 0 { 1. } else { -1. };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                center + egui::vec2(-3., 2. * sign),
                center + egui::vec2(3., 2. * sign),
                center + egui::vec2(0., -2. * sign),
            ],
            Color32::from_gray(102),
            egui::Stroke::NONE,
        ));
        if crate::native_dialog::spinner::repeat(ui, &response, ui.id().with((salt, index))) {
            let number = value.parse::<isize>().unwrap_or(1);
            *value = (number + delta).clamp(1, max as isize).to_string();
        }
    }
}
