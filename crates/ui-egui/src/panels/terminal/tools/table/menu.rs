use super::*;
pub(super) fn show(
    ui: &mut Ui,
    section: Section,
    rows: &[Vec<String>],
    selected: &std::collections::BTreeSet<usize>,
    busy: bool,
) -> Option<&'static str> {
    let mut count = 0;
    let mut running = false;
    let mut stopped = false;
    for row in selected.iter().filter_map(|i| rows.get(*i)) {
        count += 1;
        if let Some(status) = row.get(3) {
            let status = status.to_ascii_lowercase();
            running |= status.starts_with("up");
            stopped |= status.starts_with("exited");
        }
    }
    let labels: &[&str] = match section {
        Section::Docker => &[
            "启动",
            "停止",
            "重启",
            "删除",
            "强制删除",
            "查看日志",
            "查看实时日志",
            "进入终端",
            "属性",
            "复制容器ID",
            "刷新",
        ],
        Section::Ports => &["停止", "加入防火墙白名单", "复制端口", "刷新"],
        Section::Services => &[
            "启动", "停止", "重启", "启用", "禁用", "状态", "日志", "刷新",
        ],
        _ => &["刷新"],
    };
    ui.spacing_mut().item_spacing.y = 0.;
    let mut action = None;
    for &label in labels {
        if section == Section::Docker && matches!(label, "删除" | "查看日志" | "刷新")
            || section != Section::Docker && label == "刷新"
        {
            crate::compact_menu::separator(ui);
        }
        let enabled = !busy
            && if label == "刷新" {
                true
            } else if section == Section::Docker {
                match label {
                    "启动" => stopped,
                    "停止" => running,
                    "删除" => count > 0 && !running,
                    "查看日志" | "查看实时日志" | "属性" => count == 1,
                    "进入终端" => count == 1 && running,
                    _ => count > 0,
                }
            } else {
                count > 0
            };
        let display = if section == Section::Docker
            && count > 1
            && matches!(
                label,
                "启动" | "停止" | "重启" | "删除" | "强制删除" | "复制容器ID"
            ) {
            format!("{label}({count}个)")
        } else {
            label.into()
        };
        if ui
            .add_enabled(enabled, crate::compact_menu::Item::new(&display))
            .clicked()
        {
            action = Some(label);
            ui.close();
        }
    }
    action
}
#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        ctx: &egui::Context,
        rows: &[Vec<String>],
        selected: &std::collections::BTreeSet<usize>,
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<&'static str>) {
        let mut action = None;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400., 500.),
                )),
                events,
                focused: true,
                ..Default::default()
            },
            |ui| {
                action = show(ui, Section::Docker, rows, selected, false);
            },
        );
        out.textures_delta.clear();
        (out, action)
    }
    fn click(
        ctx: &egui::Context,
        rows: &[Vec<String>],
        selected: &std::collections::BTreeSet<usize>,
        label: &str,
    ) -> Option<&'static str> {
        let out = frame(ctx, rows, selected, vec![]).0;
        let p = out
            .shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    (t.galley.job.text == label).then_some(t.pos + t.galley.size() / 2.)
                } else {
                    None
                }
            })
            .unwrap();
        frame(ctx, rows, selected, vec![egui::Event::PointerMoved(p)]);
        let event = |pressed| {
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        frame(ctx, rows, selected, event(true));
        frame(ctx, rows, selected, event(false)).1
    }
    fn row(status: &str) -> Vec<String> {
        vec!["id".into(), "image".into(), "name".into(), status.into()]
    }
    #[test]
    fn running_container_menu_disables_start_and_delete_but_stop_works() {
        let ctx = egui::Context::default();
        let rows = vec![row("Up 2 hours")];
        let selected = [0].into_iter().collect();
        assert_eq!(click(&ctx, &rows, &selected, "启动"), None);
        assert_eq!(click(&ctx, &rows, &selected, "删除"), None);
        assert_eq!(click(&ctx, &rows, &selected, "停止"), Some("停止"));
    }
    #[test]
    fn mixed_multi_selection_keeps_java_count_suffix_and_disables_single_item_actions() {
        let ctx = egui::Context::default();
        let rows = vec![row("Up 2 hours"), row("Exited (0)")];
        let selected = [0, 1].into_iter().collect();
        assert_eq!(click(&ctx, &rows, &selected, "查看日志"), None);
        assert_eq!(click(&ctx, &rows, &selected, "删除(2个)"), None);
        assert_eq!(click(&ctx, &rows, &selected, "启动(2个)"), Some("启动"));
    }
}
