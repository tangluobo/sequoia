//! Original MonitorPanel row geometry and fixed 400 by 80 chart canvas.
use super::*;
pub(crate) fn render(ui: &mut Ui, workspace: &mut Workspace) {
    let colors = [
        Color32::from_rgb(255, 107, 107),
        Color32::from_rgb(78, 205, 196),
        Color32::from_rgb(69, 183, 209),
        Color32::from_rgb(150, 206, 180),
        Color32::from_rgb(155, 89, 182),
        Color32::from_rgb(231, 76, 60),
    ];
    let metrics = workspace.metrics;
    for (chart, title) in ["CPU", "内存", "网络", "磁盘IO"].into_iter().enumerate() {
        let label = if workspace.history.is_empty() {
            match chart {
                0 => "CPU: --%".into(),
                1 => "内存: --%".into(),
                2 => "网络: --".into(),
                _ => "IO: --".into(),
            }
        } else {
            match chart {
                0 => format!("CPU: {:.1}%", metrics[0]),
                1 => format!("内存: {:.1}%", metrics[1]),
                2 => format!("网络: ↓{}/s ↑{}/s", rate(metrics[2]), rate(metrics[3])),
                _ => format!("IO: ↓{}/s ↑{}/s", rate(metrics[4]), rate(metrics[5])),
            }
        };
        let row_height = (workspace.monitor_height / 4. - 12.).max(40.);
        let (row, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_height),
            egui::Sense::hover(),
        );
        let painter = ui.painter().with_clip_rect(row.intersect(ui.clip_rect()));
        painter.text(
            egui::pos2(row.left() + 8., row.center().y),
            egui::Align2::LEFT_CENTER,
            title,
            egui::FontId::proportional(11.),
            Color32::from_gray(102),
        );
        let value_x = (row.left() + 66.).min(row.right());
        let desired_width = painter
            .layout_no_wrap(
                label.clone(),
                egui::FontId::proportional(11.),
                Color32::from_gray(51),
            )
            .size()
            .x
            * 1.1;
        let value_end = (value_x + desired_width).min(row.right() - 4.).max(value_x);
        let value_rect = egui::Rect::from_min_max(
            egui::pos2(value_x, row.top()),
            egui::pos2(value_end, row.bottom()),
        );
        let mut value_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("monitor-value", chart))
                .max_rect(value_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        value_ui.set_clip_rect(value_rect.intersect(ui.clip_rect()));
        value_ui.add(egui::Label::new(RichText::new(label).strong().size(11.)).truncate());
        let chart_right = (row.right() - 4.).max(row.left());
        let chart_left = (chart_right - 400.).max(value_end + 8.).min(chart_right);
        let height = (row_height - 8.).min(80.);
        let rect = egui::Rect::from_min_size(
            egui::pos2(chart_left, row.top() + (row_height - height) / 2.),
            egui::vec2(chart_right - chart_left, height),
        );
        if rect.width() >= 65. && !workspace.history.is_empty() {
            paint_chart(ui, workspace, rect, chart, &colors);
        }
        ui.add_space(4.);
    }
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.), egui::Sense::drag());
    ui.painter().rect_filled(rect, 0, Color32::from_gray(229));
    if response.dragged() {
        workspace.monitor_height =
            (workspace.monitor_height + response.drag_delta().y).clamp(200., 800.);
    }
}
fn paint_chart(
    ui: &Ui,
    workspace: &Workspace,
    rect: egui::Rect,
    chart: usize,
    colors: &[Color32; 6],
) {
    let (start, end) = time_range(workspace);
    let plot = egui::Rect::from_min_max(
        rect.min + egui::vec2(35., 2.),
        rect.max - egui::vec2(4., 22.),
    );
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let stamp_width = painter
        .layout_no_wrap(
            "00:00".into(),
            egui::FontId::proportional(9.),
            Color32::from_gray(136),
        )
        .size()
        .x;
    let stride = if plot.width() >= 4. * (stamp_width + 8.) {
        1
    } else if plot.width() >= 2. * (stamp_width + 8.) {
        2
    } else {
        4
    };
    for i in 0..=4 {
        let y = plot.top() + plot.height() * i as f32 / 4.;
        painter.hline(
            plot.left()..=rect.right() - 2.,
            y,
            egui::Stroke::new(0.5, Color32::from_gray(224)),
        );
        painter.text(
            egui::pos2(plot.left() - 4., y),
            egui::Align2::RIGHT_CENTER,
            format!("{}%", 100 - i * 25),
            egui::FontId::proportional(9.),
            Color32::from_gray(136),
        );
        let x = plot.left() + plot.width() * i as f32 / 4.;
        painter.vline(
            x,
            plot.top()..=rect.bottom() - 20.,
            egui::Stroke::new(0.5, Color32::from_gray(224)),
        );
        if i % stride == 0 {
            let timestamp = (start + (end - start) * i as f64 / 4.) as u64;
            let galley = painter.layout_no_wrap(
                format!("{:02}:{:02}", timestamp / 60 % 60, timestamp % 60),
                egui::FontId::proportional(9.),
                Color32::from_gray(136),
            );
            let half = galley.size().x / 2.;
            let center = x.clamp(rect.left() + half + 1., rect.right() - half - 1.);
            painter.galley(
                egui::pos2(center - half, rect.bottom() - 4. - galley.size().y),
                galley,
                Color32::from_gray(136),
            );
        }
    }
    painter.vline(
        plot.left(),
        plot.top()..=rect.bottom() - 20.,
        egui::Stroke::new(1., Color32::from_gray(204)),
    );
    painter.hline(
        plot.left()..=rect.right() - 2.,
        rect.bottom() - 20.,
        egui::Stroke::new(1., Color32::from_gray(204)),
    );
    let indices: &[usize] = match chart {
        0 => &[0],
        1 => &[1],
        2 => &[2, 3],
        _ => &[4, 5],
    };
    for &index in indices {
        let points: Vec<_> = workspace
            .history
            .iter()
            .map(|sample| {
                let x = if end > start {
                    ((sample.timestamp - start) / (end - start)).clamp(0., 1.) as f32
                } else {
                    0.
                };
                egui::pos2(
                    plot.left() + plot.width() * x,
                    plot.bottom() - plot.height() * sample.values[index].clamp(0., 100.) / 100.,
                )
            })
            .collect();
        if let Some(&last) = points.last() {
            painter.circle_filled(last, 3., colors[index]);
        }
        if points.len() > 1 {
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(2., colors[index]),
            ));
        }
    }
}
fn time_range(workspace: &Workspace) -> (f64, f64) {
    let start = workspace
        .history
        .front()
        .map_or(0., |sample| sample.timestamp);
    let end = workspace
        .history
        .back()
        .map_or(start, |sample| sample.timestamp);
    (start, end)
}

fn rate(bytes: f32) -> String {
    if bytes < 1024. {
        format!("{} B", bytes as u64)
    } else if bytes < 1_048_576. {
        format!("{:.1} KB", bytes / 1024.)
    } else {
        format!("{:.1} MB", bytes / 1_048_576.)
    }
}

#[cfg(test)]
mod tests;
