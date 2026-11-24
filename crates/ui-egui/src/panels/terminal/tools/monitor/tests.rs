use super::*;
use crate::tabs::ssh::Sample;

#[test]
fn actual_monitor_curve_uses_recorded_times_and_percentage_values() {
    let context = egui::Context::default();
    let mut workspace = Workspace::default();
    for (timestamp, cpu) in [(10., 10.), (14., 50.), (30., 90.)] {
        workspace.history.push_back(Sample {
            timestamp,
            values: [cpu, 60., 30., 10., 50., 20.],
        });
    }
    let mut output = None;
    for _ in 0..2 {
        let mut frame = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900., 500.),
                )),
                ..Default::default()
            },
            |ui| render(ui, &mut workspace),
        );
        frame.textures_delta.clear();
        output = Some(frame);
    }
    let output = output.unwrap();
    for title in ["CPU", "内存", "网络", "磁盘IO"] {
        let text = output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    (text.galley.job.text == title).then_some(text)
                } else {
                    None
                }
            })
            .expect("Original metric title must be drawn");
        assert!(
            (text.pos.x - 8.).abs() < 0.1,
            "Metric names must share the original left edge"
        );
    }
    let line = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Path(path)
                if path.points.len() == 3
                    && matches!(&path.stroke.color, egui::epaint::ColorMode::Solid(color) if *color == Color32::from_rgb(255,107,107)) =>
            {
                Some(path)
            }
            _ => None,
        })
        .expect("Actual CPU curve must be painted");
    let points = &line.points;
    let ratio = (points[1].x - points[0].x) / (points[2].x - points[0].x);
    assert!(
        (ratio - 0.2).abs() < 0.001,
        "Four-second sample must not be drawn halfway through twenty-second range"
    );
    assert!(points[0].y > points[1].y && points[1].y > points[2].y);
    assert!(
        ((points[1].y - points[2].y) - 22.4).abs() < 0.01,
        "Original 80px canvas has a 56px data plot"
    );
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(t) = &shape.shape {
                Some(t.galley.job.text.as_str())
            } else {
                None
            }
        })
        .collect();
    assert!(labels.contains(&"00:10") && labels.contains(&"00:30"));
}
#[test]
fn narrow_monitor_does_not_allocate_a_chart_outside_its_content() {
    let context = egui::Context::default();
    let mut workspace = Workspace::default();
    workspace.metrics = [
        100.,
        100.,
        99_999_999.,
        99_999_999.,
        99_999_999.,
        99_999_999.,
    ];
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(180., 500.),
            )),
            ..Default::default()
        },
        |ui| {
            let bounds = ui.max_rect();
            render(ui, &mut workspace);
            assert!(ui.min_rect().right() <= bounds.right() + 0.01);
        },
    );
    output.textures_delta.clear();
    assert!(!output.shapes.is_empty());
}

#[test]
fn narrow_chart_time_labels_do_not_overlap_or_escape_the_canvas() {
    let context = egui::Context::default();
    let mut workspace = Workspace::default();
    for timestamp in [10., 14., 30.] {
        workspace.history.push_back(Sample {
            timestamp,
            values: [50.; 6],
        });
    }
    let mut out = context.run_ui(Default::default(), |ui| {
        paint_chart(
            ui,
            &workspace,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(90., 80.)),
            2,
            &[Color32::RED; 6],
        );
    });
    let mut labels: Vec<_> = out
        .shapes
        .iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                text.galley
                    .job
                    .text
                    .contains(':')
                    .then_some((text.pos.x, text.pos.x + text.galley.size().x))
            } else {
                None
            }
        })
        .collect();
    labels.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(labels.len(), 2);
    assert!(labels[0].0 >= 0. && labels[1].1 <= 90.);
    assert!(labels[0].1 + 1. <= labels[1].0);
    out.textures_delta.clear();
}
