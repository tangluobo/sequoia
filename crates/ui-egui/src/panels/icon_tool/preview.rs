use super::*;
pub(super) fn show(ui: &mut Ui, state: &mut State, save: &mut bool) {
    let (area, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 355.), egui::Sense::hover());
    let x = area.center().x - 385.;
    for i in 0..2 {
        let canvas = egui::Rect::from_min_size(
            egui::pos2(x + i as f32 * 300., area.top()),
            egui::vec2(280., 280.),
        );
        ui.painter().rect(
            canvas,
            4,
            if i == 0 {
                Color32::from_gray(245)
            } else {
                Color32::WHITE
            },
            egui::Stroke::new(1., Color32::from_gray(208)),
            egui::StrokeKind::Inside,
        );
        let texture = if i == 0 {
            state.original.as_ref()
        } else {
            state.processed.as_ref()
        };
        if let Some(texture) = texture {
            let size = texture.size_vec2();
            let scale = 270. / size.x.max(size.y);
            let image = egui::Rect::from_center_size(canvas.center(), size * scale);
            ui.painter().image(
                texture.id(),
                image,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
            if i == 1 {
                state.image_rect = Some(image);
                overlay(ui, state, image);
            }
        }
        text(
            ui,
            egui::pos2(canvas.center().x, canvas.bottom() + 6.),
            egui::Align2::CENTER_TOP,
            &format!(
                "{}：{} × {}",
                if i == 0 { "原图" } else { "透明后" },
                state.sizes[i][0],
                state.sizes[i][1]
            ),
            12.,
            Color32::from_gray(136),
        );
    }
    let control =
        egui::Rect::from_min_size(egui::pos2(x + 600., area.top()), egui::vec2(170., 340.));
    ui.painter().rect(
        control,
        4,
        Color32::WHITE,
        egui::Stroke::new(1., Color32::from_gray(224)),
        egui::StrokeKind::Inside,
    );
    let at = |y: f32, h: f32| {
        egui::Rect::from_min_size(control.min + egui::vec2(10., y), egui::vec2(150., h))
    };
    if super::super::tools::place_left(
        ui,
        at(10., 22.),
        egui::Checkbox::new(
            &mut state.crop.square,
            RichText::new("锁定正方形").size(12.),
        ),
    )
    .changed()
    {
        state.crop.reset(state.sizes[1]);
        state.sync_fields();
    }
    ui.painter().hline(
        at(41., 1.).x_range(),
        control.top() + 41.,
        egui::Stroke::new(1., Color32::from_gray(224)),
    );
    text(
        ui,
        control.min + egui::vec2(10., 52.),
        egui::Align2::LEFT_TOP,
        "距各边距离 (px)",
        12.,
        Color32::from_gray(85),
    );
    let mut edited = false;
    for (index, label) in [(1, "上"), (3, "下"), (0, "左"), (2, "右")] {
        let column = usize::from(index == 3 || index == 2);
        let row = usize::from(index == 0 || index == 2);
        let min = control.min + egui::vec2(10. + column as f32 * 79., 76. + row as f32 * 36.);
        text(
            ui,
            min + egui::vec2(0., 14.),
            egui::Align2::LEFT_CENTER,
            label,
            12.,
            Color32::from_gray(85),
        );
        edited |= ui
            .place(
                egui::Rect::from_min_size(min + egui::vec2(18., 0.), egui::vec2(53., 28.)),
                TextEdit::singleline(&mut state.fields[index])
                    .font(egui::FontId::proportional(12.)),
            )
            .changed();
    }
    if edited
        && let [Ok(left), Ok(top), Ok(right), Ok(bottom)] =
            state.fields.each_ref().map(|value| value.parse::<u32>())
    {
        state.crop.set_margins([left, top, right, bottom]);
    }
    if ui
        .place(
            at(164., 31.),
            button("应用裁切", Color32::from_rgb(33, 150, 243)),
        )
        .clicked()
        && state.ready()
    {
        let margins = state.crop.margins();
        if margins.iter().any(|&value| value > 0) {
            for (target, extra) in state.options.margins.iter_mut().zip(margins) {
                *target = target.saturating_add(extra);
            }
            state.invalidate();
        }
    }
    if ui
        .place(
            at(204., 33.),
            egui::Button::new(RichText::new("重置选区").size(14.)),
        )
        .clicked()
    {
        state.crop.reset(state.sizes[1]);
        state.sync_fields();
    }
    if ui
        .add_enabled_ui(state.ready(), |ui| {
            ui.place(
                at(247., 36.),
                button("保存", Color32::from_rgb(76, 175, 80)),
            )
        })
        .inner
        .clicked()
    {
        *save = true;
    }
    let [_, _, w, h] = state.crop.rect;
    ui.place(
        at(293., 36.),
        egui::Label::new(
            RichText::new(format!("选区：{w} × {h}\n拖动边角可调整裁切区域"))
                .size(12.)
                .color(Color32::from_gray(136)),
        )
        .wrap(),
    );
    ui.label(RichText::new("提示：选择单个文件可在此预览原图与透明后效果，棋盘格代表透明区域。拖动选区或修改右侧距离值可手动裁切。").size(12.).color(Color32::from_gray(153)));
}
fn overlay(ui: &mut Ui, state: &mut State, image: egui::Rect) {
    let [iw, ih] = state.crop.image;
    if iw == 0 || ih == 0 {
        return;
    }
    let scale = egui::vec2(image.width() / iw as f32, image.height() / ih as f32);
    let geometry = |[x, y, w, h]: [u32; 4]| {
        let selection = egui::Rect::from_min_size(
            image.min + egui::vec2(x as f32 * scale.x, y as f32 * scale.y),
            egui::vec2(w as f32 * scale.x, h as f32 * scale.y),
        );
        let points = [
            selection.left_top(),
            selection.center_top(),
            selection.right_top(),
            selection.right_center(),
            selection.right_bottom(),
            selection.center_bottom(),
            selection.left_bottom(),
            selection.left_center(),
        ];
        (selection, points)
    };
    let (selection, points) = geometry(state.crop.rect);
    let response = ui.interact(
        image.expand(5.),
        ui.id().with("icon-crop"),
        egui::Sense::drag(),
    );
    if response.drag_started()
        && let Some(point) = response.interact_pointer_pos()
    {
        let handle = points
            .iter()
            .position(|&p| egui::Rect::from_center_size(p, egui::vec2(13., 13.)).contains(point))
            .map(|i| i as u8)
            .or_else(|| selection.contains(point).then_some(8));
        state.drag = handle.map(|handle| (handle, point, state.crop.rect));
    }
    if response.dragged()
        && let (Some((handle, start, rect)), Some(point)) =
            (state.drag, response.interact_pointer_pos())
    {
        state.crop.drag(
            rect,
            handle,
            crate::tabs::icon_tool::crop::round(f64::from((point.x - start.x) / scale.x)),
            crate::tabs::icon_tool::crop::round(f64::from((point.y - start.y) / scale.y)),
        );
        state.sync_fields();
        ui.ctx().request_repaint();
    }
    if response.drag_stopped() {
        state.drag = None;
    }
    let (selection, points) = geometry(state.crop.rect);
    let blue = Color32::from_rgb(25, 118, 210);
    let painter = ui.painter().with_clip_rect(image.expand(5.));
    for (a, b) in [
        (selection.left_top(), selection.right_top()),
        (selection.right_top(), selection.right_bottom()),
        (selection.right_bottom(), selection.left_bottom()),
        (selection.left_bottom(), selection.left_top()),
    ] {
        painter.add(egui::Shape::dashed_line(
            &[a, b],
            egui::Stroke::new(2., blue),
            4.,
            4.,
        ));
    }
    for point in points {
        painter.rect(
            egui::Rect::from_center_size(point, egui::vec2(9., 9.)),
            0,
            Color32::WHITE,
            egui::Stroke::new(1.5, blue),
            egui::StrokeKind::Inside,
        );
    }
    if response.hovered()
        && let Some(point) = response.hover_pos()
    {
        let cursor = match points
            .iter()
            .position(|&p| egui::Rect::from_center_size(p, egui::vec2(13., 13.)).contains(point))
        {
            Some(0 | 4) => egui::CursorIcon::ResizeNwSe,
            Some(2 | 6) => egui::CursorIcon::ResizeNeSw,
            Some(1 | 5) => egui::CursorIcon::ResizeVertical,
            Some(3 | 7) => egui::CursorIcon::ResizeHorizontal,
            _ => egui::CursorIcon::Move,
        };
        ui.ctx().set_cursor_icon(cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(ctx: &egui::Context, state: &mut State, events: Vec<egui::Event>) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600., 500.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                overlay(
                    ui,
                    state,
                    egui::Rect::from_min_size(egui::pos2(100., 100.), egui::vec2(270., 216.)),
                )
            },
        );
        output.textures_delta.clear();
        output
    }
    #[test]
    fn edge_handle_accepts_its_outer_half_and_repaints_resized_selection_in_the_same_frame() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        state.crop.square = false;
        state.crop.reset([100, 80]);
        frame(&ctx, &mut state, vec![]);
        let start = egui::pos2(373., 319.);
        frame(
            &ctx,
            &mut state,
            vec![
                egui::Event::PointerMoved(start),
                egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        let output = frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(start - egui::vec2(27., 27.))],
        );
        assert_eq!(state.crop.rect, [0, 0, 90, 70]);
        assert!(output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Rect(rect) if rect.rect.center().distance(egui::pos2(343.,289.))<0.01)),"resize must paint current, not previous, selection coordinates");
    }
}
