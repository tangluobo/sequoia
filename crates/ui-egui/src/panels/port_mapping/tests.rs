use super::*;
use crate::app::tests::native_bridge::Bridge;

#[test]
fn toolbar_and_row_buttons_keep_java_geometry_in_a_scrolling_narrow_panel() -> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let mut state = State::new(
        directory.path().join("mappings.json"),
        None,
        directory.path().join("known"),
    );
    let mut host = tomato_core::Connection::new("SSH");
    host.name = "较长的 SSH 主机名称".repeat(8);
    for index in 0..32 {
        state.mappings.push(Mapping {
            name: format!("映射-{index} 带换行\n第二行"),
            ssh_host_id: host.id.clone(),
            ..Default::default()
        });
    }
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let hosts = [host];
    let mut frame = |time| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(440., 250.),
                )),
                time: Some(time),
                ..Default::default()
            },
            |ui| {
                render(ui, &mut state, &hosts);
            },
        );
        output.textures_delta.clear();
        output
    };
    frame(0.);
    let output = frame(1.);
    let shapes = &output.shapes;
    let toolbar = ["添加", "修改", "删除", "启动", "停止"];
    for (label, left) in toolbar.into_iter().zip([10., 58., 108., 158., 206.]) {
        let (shape, text) = shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => Some((shape, text)),
                _ => None,
            })
            .unwrap();
        assert_eq!(text.galley.rows.len(), 1, "{label}");
        assert_eq!(shape.clip_rect.left(), left, "{label}");
        assert!((shape.clip_rect.center().y - 21.).abs() < 0.01, "{label}");
    }
    let action_column = shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "操作" => Some(shape.clip_rect),
            _ => None,
        })
        .unwrap();
    assert!(
        action_column.right() < 440.,
        "header must reserve the scrollbar"
    );
    let actions: Vec<_> = shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if rect.rect.width() == ACTION_WIDTH
                    && rect.rect.top() >= 71.
                    && rect.fill == Color32::from_rgb(25, 118, 210) =>
            {
                Some((shape.clip_rect, rect.rect))
            }
            _ => None,
        })
        .take(3)
        .collect();
    assert_eq!(actions.len(), 3);
    for (index, (clip, rect)) in actions.into_iter().enumerate() {
        assert_eq!(rect.top(), 71. + index as f32 * ROW_HEIGHT);
        assert!((rect.left() - action_column.left() - 6.).abs() < 0.01);
        assert!(rect.right() <= action_column.right());
        assert!(clip.contains_rect(rect), "row button must be fully visible");
    }
    assert!(shapes.iter().any(|shape| matches!(&shape.shape,
        egui::Shape::Text(text) if text.galley.job.text.starts_with("映射-0 带换行 ")
            && text.galley.rows.len() == 1)));
    Ok(())
}

#[test]
fn toolbar_blocks_duplicate_start_and_matches_java_single_and_mixed_selection_rules()
-> anyhow::Result<()> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("port-mappings.json");
    // No executor is driven: pending handshakes remain pending without network I/O.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut state = State::new(
        path.clone(),
        Some(runtime.handle().clone()),
        directory.path().join("known"),
    );
    let mut host = tomato_core::Connection::new("SSH");
    host.set("host", "127.0.0.1");
    for i in 0..2 {
        state.mappings.push(Mapping {
            name: format!("mapping-{i}"),
            ssh_host_id: host.id.clone(),
            local_port: 45000 + i,
            remote_port: 45002,
            ..Default::default()
        });
    }
    let hosts = [host];
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let bridge = Bridge::new(&ctx);
    fn buttons(
        ctx: &egui::Context,
        bridge: &Bridge,
        state: &mut State,
        hosts: &[tomato_core::Connection],
        expected: [bool; 4],
    ) {
        bridge.root_frame(ctx, vec![], vec![], |ui| {
            let labels = ["修改", "删除", "启动", "停止"];
            let ids = labels.map(|label| ui.id().with(("mapping-toolbar", label)));
            render(ui, state, hosts);
            for ((id, label), enabled) in ids.into_iter().zip(labels).zip(expected) {
                assert_eq!(
                    ctx.read_response(id)
                        .expect("actual toolbar widget")
                        .enabled(),
                    enabled,
                    "{label}: selection {:?}, pending {:?}, active {:?}",
                    state.selected,
                    state
                        .mappings
                        .iter()
                        .map(|m| state.busy(&m.id))
                        .collect::<Vec<_>>(),
                    state.active.keys().collect::<Vec<_>>()
                );
            }
        });
    }
    buttons(
        &ctx,
        &bridge,
        &mut state,
        &hosts,
        [false, false, false, false],
    );
    state.selected.insert(0);
    buttons(&ctx, &bridge, &mut state, &hosts, [true, true, true, false]);
    state.selected.insert(1);
    buttons(
        &ctx,
        &bridge,
        &mut state,
        &hosts,
        [false, true, true, false],
    );
    click(&ctx, &bridge, &mut state, &hosts, "启动");
    assert!(state.mappings.iter().all(|m| state.busy(&m.id)));
    buttons(
        &ctx,
        &bridge,
        &mut state,
        &hosts,
        [false, false, false, true],
    );
    let second = state.mappings[1].id.clone();
    state.stop(&second);
    buttons(
        &ctx,
        &bridge,
        &mut state,
        &hosts,
        [false, false, false, false],
    );
    state.selected.remove(&1);
    buttons(
        &ctx,
        &bridge,
        &mut state,
        &hosts,
        [false, false, false, true],
    );
    click(&ctx, &bridge, &mut state, &hosts, "停止");
    assert!(state.mappings.iter().all(|m| !state.busy(&m.id)));
    let mapping = state.mappings[0].clone();
    let id = mapping.id.clone();
    let forward = {
        let _guard = runtime.enter();
        tomato_services::terminal::ssh::forwarding::Forward::start(
            mapping,
            hosts[0].clone(),
            directory.path().join("known"),
            || {},
        )?
    };
    state.active.insert(id.clone(), forward);
    buttons(
        &ctx,
        &bridge,
        &mut state,
        &hosts,
        [false, false, false, true],
    );
    click(&ctx, &bridge, &mut state, &hosts, "停止");
    assert!(!state.active.contains_key(&id));
    buttons(&ctx, &bridge, &mut state, &hosts, [true, true, true, false]);
    assert!(!path.exists());
    Ok(())
}
fn frame(
    ctx: &egui::Context,
    bridge: &Bridge,
    state: &mut State,
    hosts: &[tomato_core::Connection],
    root: Vec<egui::Event>,
    child: Vec<egui::Event>,
) -> egui::FullOutput {
    bridge.root_frame(ctx, root, child, |ui| {
        render(ui, state, hosts);
    })
}
fn click(
    ctx: &egui::Context,
    bridge: &Bridge,
    state: &mut State,
    hosts: &[tomato_core::Connection],
    label: &str,
) {
    frame(ctx, bridge, state, hosts, vec![], vec![]);
    let output = frame(ctx, bridge, state, hosts, vec![], vec![]);
    let child = state.editor.is_some();
    let pos = if child {
        bridge.text(label).unwrap()
    } else {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing actual control: {label}"))
    };
    for pressed in [true, false] {
        let events = vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            },
        ];
        let (root, native) = if child {
            (vec![], events)
        } else {
            (events, vec![])
        };
        frame(ctx, bridge, state, hosts, root, native);
    }
}
#[test]
fn actual_mapping_add_edit_cancel_save_delete_controls_preserve_legacy_file() -> anyhow::Result<()>
{
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("port-mappings.json");
    let mut state = State::new(path.clone(), None, directory.path().join("known_hosts"));
    let mut host = tomato_core::Connection::new("SSH");
    host.name = "owned-host".into();
    let hosts = [host];
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let bridge = Bridge::new(&ctx);
    click(&ctx, &bridge, &mut state, &hosts, "添加");
    assert!(state.editor.is_some());
    click(&ctx, &bridge, &mut state, &hosts, "取消");
    assert!(state.editor.is_none());
    assert!(!path.exists());
    click(&ctx, &bridge, &mut state, &hosts, "添加");
    let editor = state.editor.as_mut().unwrap();
    editor.mapping.name = "owned-L".into();
    editor.mapping.ssh_host_id = hosts[0].id.clone();
    editor.local = "45678".into();
    editor.remote = "12345".into();
    click(&ctx, &bridge, &mut state, &hosts, "确定");
    assert!(state.editor.is_none());
    assert_eq!(state.mappings.len(), 1);
    let (_, saved) = tomato_services::terminal::ssh::forwarding::Repository::load(path.clone())?;
    assert_eq!(saved[0].name, "owned-L");
    assert_eq!(saved[0].local_port, 45678);
    state.selected.insert(0);
    click(&ctx, &bridge, &mut state, &hosts, "修改");
    state.editor.as_mut().unwrap().mapping.direction = "R".into();
    click(&ctx, &bridge, &mut state, &hosts, "保存");
    assert_eq!(state.mappings[0].direction, "R");
    click(&ctx, &bridge, &mut state, &hosts, "删除");
    assert!(state.delete.is_some());
    click(&ctx, &bridge, &mut state, &hosts, "取消");
    assert_eq!(state.mappings.len(), 1);
    click(&ctx, &bridge, &mut state, &hosts, "删除");
    click(&ctx, &bridge, &mut state, &hosts, "确定");
    assert!(state.mappings.is_empty());
    assert_eq!(std::fs::read_to_string(path)?.trim(), "[]");
    Ok(())
}
