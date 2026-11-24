use crate::tabs::ssh::Workspace;
use egui::{Color32, RichText};
use tomato_services::files::object_copy::Phase;
pub(super) fn render(ctx: &egui::Context, workspace: &mut Workspace) {
    let capture = std::env::var_os("TOMATO_UI_CAPTURE").is_some()
        && std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("object_copy");
    if workspace.copy_window_visible
        && let Some(owner) = &workspace.object_copy
    {
        let job = owner.0.clone();
        super::editor::ensure_bold_cjk(ctx);
        let report = job.snapshot();
        if matches!(
            report.phase,
            Phase::Waiting | Phase::Scanning | Phase::Copying
        ) {
            let id = egui::ViewportId::from_hash_of((
                "object-copy",
                std::sync::Arc::as_ptr(&job) as usize,
            ));
            let mut builder = egui::ViewportBuilder::default()
                .with_title("粘贴文件...")
                .with_inner_size(workspace.copy_inner_size.unwrap_or(egui::vec2(500., 200.)))
                .with_resizable(false);
            if capture {
                builder = builder.with_active(false);
                if let Some(path) =
                    std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from)
                {
                    let state = serde_json::json!({"phase":format!("{:?}",report.phase),"total":report.total,"succeeded":report.succeeded,"failed":report.failed,"current":report.current,"transferred":report.transferred,"fileBytes":report.file_bytes,"stage":report.transfer_stage});
                    let _ = std::fs::write(path.with_extension("state.json"), state.to_string());
                }
                ctx.data_mut(|data| data.insert_temp(egui::Id::new("tomato-capture-viewport"), id));
            }
            if !workspace.copy_window_positioned {
                if let Some(rect) = ctx.input(|input| input.viewport().inner_rect) {
                    builder = builder.with_position(rect.center() - egui::vec2(250., 100.));
                }
                workspace.copy_window_positioned = true;
            }
            ctx.show_viewport_immediate(id, builder, |ui, _| {
                if workspace.copy_inner_size.is_none()
                    && let Some((inner, outer)) = ui.input(|input| {
                        input.viewport().inner_rect.zip(input.viewport().outer_rect)
                    })
                {
                    let chrome = (outer.size() - inner.size()).max(egui::Vec2::ZERO);
                    let size = egui::vec2(500., 200.) - chrome;
                    workspace.copy_inner_size = Some(size);
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                }
                ui.painter().rect_filled(ui.max_rect(),0.,Color32::from_gray(244));
                if capture && let Some(image)=ui.input(|input|input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot {viewport_id,image,..}=event && *viewport_id==id {Some(image.clone())} else {None}
                })) {
                    ui.ctx().data_mut(|data|data.insert_temp(egui::Id::new("tomato-capture-image"),image));
                    if let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(std::path::PathBuf::from) {
                        let state=serde_json::json!({"phase":format!("{:?}",report.phase),"total":report.total,"succeeded":report.succeeded,"failed":report.failed,"current":report.current,"transferred":report.transferred,"fileBytes":report.file_bytes,"stage":report.transfer_stage});
                        let _=std::fs::write(path.with_extension("state.json"),state.to_string());
                    }
                    ui.ctx().request_repaint_of(egui::ViewportId::ROOT);
                }
                ui.style_mut().override_font_id = Some(egui::FontId::proportional(12.));
                if ui.input(|input| input.viewport().close_requested()) {
                    job.cancel();
                    workspace.copy_window_visible = false;
                    return;
                }
                {
                    let origin = ui.max_rect().min;
                    let width = ui.max_rect().width() - 30.;
                    let at = |x, y, w, h| egui::Rect::from_min_size(origin + egui::vec2(x, y), egui::vec2(w, h));
                    label(ui,at(15.,15.,width,19.),"正在粘贴...".into(),egui::FontId::new(14.,egui::FontFamily::Name("md-bold".into())),Color32::BLACK);
                    let text = if job.is_cancelled() {
                        "正在取消...".into()
                    } else if report.phase == Phase::Waiting {
                        "等待其他复制任务完成...".into()
                    } else if report.phase == Phase::Scanning {
                        if report.current.is_empty() {
                            "正在扫描目录结构...".into()
                        } else {
                            format!("正在扫描: {}", report.current)
                        }
                    } else {
                        format!(
                            "正在复制: {} ({}/{})",
                            report.current,
                            report.succeeded + report.failed,
                            report.total
                        )
                    };
                    label(ui,at(15.,44.,width,15.),text,egui::FontId::proportional(12.),Color32::BLACK);
                    let done = report.succeeded + report.failed;
                    let progress=if report.total==0 {0.} else {done as f32/report.total as f32};
                    let rect=at(15.,69.,width,17.);
                    ui.painter().rect(rect,3.,Color32::from_gray(234),egui::Stroke::new(1.,Color32::from_gray(180)),egui::StrokeKind::Inside);
                    if progress>0. {let inner=rect.shrink(2.);ui.painter().rect_filled(egui::Rect::from_min_size(inner.min,egui::vec2(inner.width()*progress.clamp(0.,1.),inner.height())),2.,Color32::from_rgb(53,146,203));}
                    let detail = if report.phase == Phase::Scanning {
                        format!("已发现 {} 个文件", report.total)
                    } else if report.transfer_stage.is_empty() {
                        String::new()
                    } else if let Some(size) = report.file_bytes.filter(|s| *s > 0) {
                        format!(
                            "{}中: {} / {} ({:.0}%)",
                            report.transfer_stage,
                            human(report.transferred),
                            human(size),
                            report.transferred as f64 / size as f64 * 100.
                        )
                    } else {
                        format!("{}中: {}", report.transfer_stage, human(report.transferred))
                    };
                    label(ui,at(15.,96.,width,15.),detail,egui::FontId::proportional(11.),Color32::from_gray(102));
                    ui.add_enabled_ui(!job.is_cancelled(), |ui| {
                        ui.spacing_mut().button_padding=egui::vec2(8.,4.);
                        if ui.place(at(width+15.-42.,120.8,42.,25.),egui::Button::new("取消").wrap_mode(egui::TextWrapMode::Extend).min_size(egui::vec2(42.,25.))).clicked() {
                            job.cancel();
                        }
                    });
                }
            });
        }
    }
    if let Some(report) = workspace.copy_errors.clone() {
        let failed = report.phase == Phase::Failed;
        let id = egui::ViewportId::from_hash_of((
            "object-copy-errors",
            std::sync::Arc::as_ptr(&report) as usize,
        ));
        ctx.show_viewport_immediate(
            id,
            egui::ViewportBuilder::default()
                .with_title(if failed {
                    "粘贴失败"
                } else {
                    "部分粘贴失败"
                })
                .with_inner_size([500., 260.])
                .with_resizable(false),
            |ui, _| {
                ui.painter().rect_filled(ui.max_rect(), 0., Color32::WHITE);
                if ui.input(|input| input.viewport().close_requested()) {
                    workspace.copy_errors = None;
                    return;
                }
                egui::Frame::new().inner_margin(15).show(ui, |ui| {
                    ui.label(
                        RichText::new(if failed {
                            "复制失败".into()
                        } else {
                            format!("成功 {} 个, 失败 {} 个", report.succeeded, report.failed)
                        })
                        .strong(),
                    );
                    egui::ScrollArea::vertical()
                        .max_height(160.)
                        .show(ui, |ui| {
                            if failed {
                                ui.label(&report.current);
                            }
                            for (error, count) in &report.errors {
                                ui.label(format!("[{count}次] {error}"));
                            }
                            if report.other_errors > 0 {
                                ui.label(format!("...等其他 {} 次错误", report.other_errors));
                            }
                        });
                    if ui.button("确定").clicked() {
                        workspace.copy_errors = None;
                    }
                });
            },
        );
    }
}
fn label(ui: &egui::Ui, rect: egui::Rect, text: String, font: egui::FontId, color: Color32) {
    let mut job = egui::text::LayoutJob::simple(text, font, color, rect.width());
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    ui.painter().galley(rect.min, galley, color);
}
fn human(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.),
        1_048_576..1_073_741_824 => format!("{:.1} MB", bytes as f64 / 1_048_576.),
        _ => format!("{:.1} GB", bytes as f64 / 1_073_741_824.),
    }
}
