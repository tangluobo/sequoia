#[test]
fn readonly_sql_information_keeps_selection_and_copy_without_undo_snapshots() {
    let ctx = egui::Context::default();
    let text = "SELECT '中文';\n > OK\n > 时间: 0.001s\n > 行数: 1\n".repeat(512);
    let id = egui::Id::new("readonly-sql-info-regression");
    let mut copied = Vec::new();
    for frame in 0..4 {
        let mut readonly = text.as_str();
        let command = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        let events = if frame == 2 {
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: command,
                },
                egui::Event::Copy,
            ]
        } else {
            Vec::new()
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(f64::from(frame)),
                events,
                ..Default::default()
            },
            |ui| {
                ui.memory_mut(|memory| memory.request_focus(id));
                let edit = egui::TextEdit::multiline(&mut readonly).id(id).show(ui);
                assert_eq!(
                    edit.state.trim_undo_memory(usize::MAX),
                    0,
                    "read-only results must not retain whole-text undo snapshots"
                );
            },
        );
        for command in &output.platform_output.commands {
            if let egui::OutputCommand::CopyText(value) = command {
                copied.push(value.clone());
            }
        }
        output.textures_delta.clear();
    }
    assert_eq!(copied, [text]);
}
