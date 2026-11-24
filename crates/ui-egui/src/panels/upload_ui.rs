use crate::tabs::ssh::Workspace;
use egui::*;
use tomato_services::files::object_copy::Phase;

pub(super) fn render(ctx: &Context, workspace: &mut Workspace) {
    if let Some(upload) = &mut workspace.upload {
        super::editor::ensure_bold_cjk(ctx);
        let report = upload.owner.0.snapshot();
        let id = ViewportId::from_hash_of((
            "file-upload-progress",
            std::sync::Arc::as_ptr(&upload.owner.0) as usize,
        ));
        let mut builder = ViewportBuilder::default()
            .with_title(if report.total == 0 {
                "上传文件".into()
            } else {
                format!("上传文件 ({} 个)", report.total)
            })
            .with_inner_size(vec2(484., 151.))
            .with_resizable(false)
            .with_active(false);
        if !upload.positioned {
            if let Some(parent) = ctx.input(|input| input.viewport().inner_rect) {
                builder = builder.with_position(parent.center() - vec2(242., 75.5));
            }
            upload.positioned = true;
        }
        ctx.show_viewport_immediate(id, builder, |ui, _| {
            if ui.input(|input| input.viewport().close_requested()) {
                upload.owner.0.cancel();
                ui.ctx().send_viewport_cmd(ViewportCommand::CancelClose);
            }
            let bounds = ui.max_rect();
            ui.painter().rect_filled(bounds, 0., Color32::WHITE);
            let at = |x, y, w, h| Rect::from_min_size(bounds.min + vec2(x, y), vec2(w, h));
            let width = bounds.width() - 30.;
            let fixture = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(Id::new("file-drop-preview-installed")))
                .unwrap_or(false);
            if fixture {
                ui.ctx().data_mut(|data| {
                    data.insert_temp(Id::new("file-drop-progress-viewport"), id);
                    data.insert_temp(
                        Id::new("file-drop-cancel-point"),
                        at(bounds.width() - 75., 112., 60., 24.).center(),
                    );
                    data.insert_temp(
                        Id::new("file-drop-transfer-started"),
                        report.phase == Phase::Copying && report.transferred > 0,
                    );
                    if std::env::var("TOMATO_FILE_DROP_CASE").as_deref() == Ok("progress") {
                        data.insert_temp(Id::new("tomato-capture-viewport"), id);
                    }
                });
                if let Some(image) = ui.input(|input| {
                    input.events.iter().find_map(|event| {
                        if let Event::Screenshot {
                            viewport_id, image, ..
                        } = event
                            && *viewport_id == id
                        {
                            Some(image.clone())
                        } else {
                            None
                        }
                    })
                }) {
                    ui.ctx()
                        .data_mut(|data| data.insert_temp(Id::new("tomato-capture-image"), image));
                }
            }
            label(
                ui,
                at(15., 15., width, 17.),
                "正在上传...",
                14.,
                Color32::BLACK,
            );
            let cancelled = upload.owner.0.is_cancelled();
            let text = if cancelled {
                "正在取消...".into()
            } else if report.phase == Phase::Waiting {
                "等待其他上传任务完成...".into()
            } else if report.phase == Phase::Scanning {
                "准备上传...".into()
            } else {
                format!(
                    "正在上传: {} ({}/{})",
                    report.current,
                    (report.succeeded + report.failed + 1).min(report.total),
                    report.total
                )
            };
            label(
                ui,
                at(15., 42., width, 15.),
                &text,
                12.,
                Color32::from_gray(51),
            );
            let done = report.succeeded + report.failed;
            let progress = if let Some(total) = report.total_bytes.filter(|total| *total > 0) {
                (report
                    .completed_bytes
                    .saturating_add(report.transferred)
                    .min(total)) as f32
                    / total as f32
            } else if report.total > 0 {
                done as f32 / report.total as f32
            } else {
                0.
            };
            ui.place(
                at(15., 67., width, 12.),
                ProgressBar::new(progress)
                    .desired_width(width)
                    .desired_height(12.)
                    .fill(Color32::from_rgb(7, 193, 96))
                    .corner_radius(0),
            );
            let file = if done >= report.total && report.total > 0 {
                "完成".into()
            } else if let Some(size) = report.file_bytes.filter(|size| *size > 0) {
                format!("{} / {}", bytes(report.transferred.min(size)), bytes(size))
            } else {
                "处理中...".into()
            };
            label(
                ui,
                at(15., 89., width, 13.),
                &format!("{file} · 总进度 {:.1}%", progress * 100.),
                11.,
                Color32::from_gray(102),
            );
            if crate::native_dialog::button(
                ui,
                at(bounds.width() - 75., 112., 60., 24.),
                "取消",
                false,
            )
            .clicked()
            {
                upload.owner.0.cancel();
            }
        });
    }
    if let Some(report) = workspace.upload_report.clone() {
        let mut dismiss = false;
        Window::new(if report.phase == Phase::Failed {
            "上传失败"
        } else {
            "部分上传失败"
        })
        .id(Id::new((
            "file-upload-errors",
            std::sync::Arc::as_ptr(&report) as usize,
        )))
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label(if report.phase == Phase::Failed {
                report.current.clone()
            } else {
                format!("成功 {} 个, 失败 {} 个", report.succeeded, report.failed)
            });
            for (error, count) in &report.errors {
                ui.label(format!("{error} ({count})"));
            }
            if report.other_errors > 0 {
                ui.label(format!("另有 {} 项错误", report.other_errors));
            }
            dismiss = ui.button("确定").clicked();
        });
        if dismiss {
            workspace.upload_report = None;
        }
    }
}
fn label(ui: &Ui, rect: Rect, text: &str, size: f32, color: Color32) {
    let font = if size == 14. {
        FontId::new(size, FontFamily::Name("md-bold".into()))
    } else {
        FontId::proportional(size)
    };
    let mut job = egui::text::LayoutJob::simple(text.into(), font, color, rect.width());
    job.wrap.max_rows = 1;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter_at(rect).galley(
        pos2(rect.left(), rect.center().y - galley.size().y / 2.),
        galley,
        color,
    );
}
fn bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024. * 1024. * 1024.))
    }
}
