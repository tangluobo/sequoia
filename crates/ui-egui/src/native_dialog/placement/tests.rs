use super::*;
use egui::{RawInput, ViewportInfo};

fn frame(ctx: &Context, input: RawInput, render: impl FnMut(&mut egui::Ui)) -> egui::FullOutput {
    let mut output = ctx.run_ui(input, render);
    output.textures_delta.clear();
    output
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}
#[test]
fn native_dialog_chooses_owner_display_work_area_instead_of_window_center() {
    let ctx = Context::default();
    let dialog = ViewportId::from_hash_of("placement-child");
    let mut input = RawInput::default();
    let owner = rect(-1500., 150., 700., 500.);
    let root = input.viewports.get_mut(&ViewportId::ROOT).unwrap();
    root.outer_rect = Some(owner);
    root.monitor_work_area_px = Some(rect(-1920., 40., 1920., 1000.));
    let mut position = None;
    let _ = frame(&ctx, input, |ui| {
        position = prepare(
            ui.ctx(),
            dialog,
            ViewportBuilder::default(),
            true,
            Some(owner),
            egui::vec2(500., 450.),
        )
        .position;
    });
    assert_eq!(position, Some(egui::pos2(-1210., 315.)));
    assert_ne!(position, Some(owner.center() - egui::vec2(250., 225.)));
}
#[test]
fn warning_uses_its_wizard_owner_when_the_main_window_spans_both_displays() {
    let ctx = Context::default();
    let wizard = ViewportId::from_hash_of("placement-wizard");
    let warning = ViewportId::from_hash_of("placement-warning");
    let mut input = RawInput::default();
    let root = input.viewports.get_mut(&ViewportId::ROOT).unwrap();
    root.outer_rect = Some(rect(-1600., 0., 3500., 1080.));
    root.monitor_work_area_px = Some(rect(0., 0., 1920., 1040.));
    let owner = rect(-1500., 200., 580., 530.);
    input.viewports.insert(
        wizard,
        ViewportInfo {
            parent: Some(ViewportId::ROOT),
            outer_rect: Some(owner),
            monitor_work_area_px: Some(rect(-1920., 0., 1920., 1040.)),
            ..Default::default()
        },
    );
    let mut position = None;
    let _ = frame(&ctx, input, |ui| {
        assert_eq!(owner_rect(ui.ctx(), wizard), Some(owner));
        position = prepare(
            ui.ctx(),
            warning,
            ViewportBuilder::default(),
            true,
            Some(owner),
            egui::vec2(418., 101.),
        )
        .position;
    });
    assert_eq!(position, Some(egui::pos2(-1169., 469.5)));
}
#[test]
fn native_size_correction_converts_physical_work_area_with_child_dpi_only_once() {
    let ctx = Context::default();
    let dialog = ViewportId::from_hash_of("placement-scaled-child");
    ctx.data_mut(|data| {
        data.insert_temp(
            key(dialog),
            Pending {
                work_px: rect(-3840., 60., 3840., 2100.),
                attempts: 6,
                last: None,
            },
        )
    });
    let mut input = RawInput {
        viewport_id: dialog,
        ..Default::default()
    };
    input.viewports.insert(
        dialog,
        ViewportInfo {
            parent: Some(ViewportId::ROOT),
            native_pixels_per_point: Some(2.),
            outer_rect: Some(rect(0., 0., 430., 160.)),
            ..Default::default()
        },
    );
    let output = frame(&ctx, input.clone(), |ui| correct(ui, dialog));
    let position = output.viewport_output[&dialog]
        .commands
        .iter()
        .find_map(|cmd| {
            if let egui::ViewportCommand::OuterPosition(p) = cmd {
                Some(*p)
            } else {
                None
            }
        });
    assert_eq!(position, Some(egui::pos2(-1175., 475.)));
    assert!(
        ctx.data_mut(|data| data.get_temp::<Pending>(key(dialog)))
            .is_some()
    );
    input.viewports.get_mut(&dialog).unwrap().outer_rect = Some(rect(-1175., 475., 430., 160.));
    let output = frame(&ctx, input.clone(), |ui| correct(ui, dialog));
    assert!(
        !output.viewport_output[&dialog]
            .commands
            .iter()
            .any(|cmd| matches!(cmd, egui::ViewportCommand::OuterPosition(_)))
    );
    assert!(
        ctx.data_mut(|data| data.get_temp::<Pending>(key(dialog)))
            .is_none()
    );
    input.viewports.get_mut(&dialog).unwrap().outer_rect = Some(rect(200., 200., 430., 160.));
    let output = frame(&ctx, input, |ui| correct(ui, dialog));
    assert!(
        !output.viewport_output[&dialog]
            .commands
            .iter()
            .any(|cmd| matches!(cmd, egui::ViewportCommand::OuterPosition(_)))
    );
}
#[test]
fn crossing_dpi_during_initial_show_remeasures_before_settling() {
    let ctx = Context::default();
    let dialog = ViewportId::from_hash_of("placement-cross-dpi");
    ctx.data_mut(|d| {
        d.insert_temp(
            key(dialog),
            Pending {
                work_px: rect(-3840., 60., 3840., 2100.),
                attempts: 6,
                last: None,
            },
        )
    });
    let mut input = RawInput {
        viewport_id: dialog,
        ..Default::default()
    };
    input.viewports.insert(
        dialog,
        ViewportInfo {
            parent: Some(ViewportId::ROOT),
            native_pixels_per_point: Some(1.),
            outer_rect: Some(rect(0., 0., 430., 160.)),
            ..Default::default()
        },
    );
    let _ = frame(&ctx, input.clone(), |ui| correct(ui, dialog));
    input
        .viewports
        .get_mut(&dialog)
        .unwrap()
        .native_pixels_per_point = Some(2.);
    let output = frame(&ctx, input, |ui| correct(ui, dialog));
    assert!(output.viewport_output[&dialog].commands.iter().any(
        |cmd| matches!(cmd,egui::ViewportCommand::OuterPosition(p)if *p==egui::pos2(-1175.,475.))
    ));
}

#[test]
fn oversized_windows_clamp_to_work_area_origin_and_missing_geometry_keeps_fallback() {
    assert_eq!(
        centered(rect(-100., 20., 600., 400.), egui::vec2(900., 700.)),
        egui::pos2(-100., 20.)
    );
    let ctx = Context::default();
    let dialog = ViewportId::from_hash_of("placement-no-native-geometry");
    let owner = rect(100., 200., 800., 600.);
    let mut position = None;
    let _ = frame(&ctx, RawInput::default(), |ui| {
        position = prepare(
            ui.ctx(),
            dialog,
            ViewportBuilder::default(),
            true,
            Some(owner),
            egui::vec2(400., 200.),
        )
        .position
    });
    assert_eq!(position, Some(egui::pos2(300., 400.)));
}
