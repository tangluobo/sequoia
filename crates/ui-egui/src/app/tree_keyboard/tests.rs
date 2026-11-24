use crate::app::tree_rename::tests::Driver;
use egui::{Key, Modifiers, PointerButton};
#[test]
fn unloaded_folder_arrow_loads_real_children_then_keyboard_expands_and_collapses()
-> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let folder = d.directory.path().join("files").join("folder");
    assert!(
        !d.app.local_trees[&d.local.id]
            .directories
            .contains_key(&folder)
    );
    let label = d.point("folder");
    let out = d.frame(vec![]);
    let arrow = out
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Path(path) if path.points.len() == 3 => {
                let rect = egui::Rect::from_points(&path.points);
                ((rect.center().y - label.y).abs() < 2. && rect.right() < label.x)
                    .then_some(rect.center())
            }
            _ => None,
        })
        .expect("unloaded folder must expose its real expansion arrow");
    d.click(arrow, PointerButton::Primary, Modifiers::NONE);
    d.drain()?;
    assert!(
        d.app.local_trees[&d.local.id]
            .directories
            .contains_key(&folder)
    );
    d.select("nested");
    d.key(Key::ArrowRight);
    d.drain()?;
    d.point("inside.md");
    assert!(
        d.app.local_trees[&d.local.id]
            .directories
            .contains_key(&folder.join("nested"))
    );
    d.key(Key::ArrowLeft);
    assert!(
        d.app.pending.is_empty(),
        "collapsing cached nodes must not fetch again"
    );
    assert!(
        d.app.collapsed.contains(&crate::app::local_tree::key(
            &d.local.id,
            &folder.join("nested")
        )),
        "selected={:?}, collapsed={:?}, focused={:?}",
        d.app.selected_connection,
        d.app.collapsed,
        d.app.context.memory(|m| m.focused())
    );
    d.key(Key::ArrowLeft);
    assert_eq!(
        d.app.selected_connection,
        Some(crate::app::local_tree::key(&d.local.id, &folder))
    );
    Ok(())
}
#[test]
fn connection_tree_arrows_range_selection_and_blank_menu_use_real_controls() -> anyhow::Result<()> {
    let mut d = Driver::new()?;
    let original = std::fs::read(d.directory.path().join("connections.json"))?;
    d.select("first.md");
    d.frame(vec![egui::Event::Key {
        key: Key::ArrowDown,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::SHIFT,
    }]);
    assert_eq!(d.app.local_selection.len(), 2);
    d.key(Key::Home);
    assert_eq!(
        d.app.selected_connection.as_deref(),
        Some(d.app.store.connections[0].id.as_str())
    );
    d.key(Key::End);
    assert!(
        d.app
            .selected_connection
            .as_deref()
            .is_some_and(|key| key.starts_with("query")
                || key.starts_with("backup")
                || key.contains("function")
                || key.contains("folder")),
        "last visible tree row selected: {:?}",
        d.app.selected_connection
    );
    d.click(
        egui::pos2(280., 1010.),
        PointerButton::Secondary,
        Modifiers::NONE,
    );
    let output = d.frame(vec![]);
    let labels: Vec<_> = output
        .shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.job.text.as_str()),
            _ => None,
        })
        .collect();
    for label in ["新建目录", "新建连接", "导入连接", "导出连接"] {
        assert!(
            labels.contains(&label),
            "missing root action {label}: {labels:?}"
        );
    }
    assert_eq!(
        std::fs::read(d.directory.path().join("connections.json"))?,
        original
    );
    Ok(())
}

#[test]
fn pending_directory_collapse_survives_reply_and_empty_directories_are_cached() -> anyhow::Result<()>
{
    let mut d = Driver::new()?;
    let folder = d.directory.path().join("files").join("folder");
    let nested = folder.join("nested");
    let key = crate::app::local_tree::key(&d.local.id, &folder);
    d.select("folder");
    d.key(Key::ArrowRight);
    assert!(d.app.local_trees[&d.local.id].loading(&folder));
    d.key(Key::ArrowLeft);
    assert!(d.app.collapsed.contains(&key));
    d.drain()?;
    assert!(
        d.app.collapsed.contains(&key),
        "completion must retain the user's collapse"
    );
    d.key(Key::ArrowRight);
    assert!(!d.app.collapsed.contains(&key));
    assert!(
        d.app.pending.is_empty(),
        "opening cached directories does not read again"
    );
    d.point("nested");
    let empty = nested.join("empty");
    std::fs::create_dir(&empty)?;
    d.app.local_tree_open(&d.local, Some(empty.clone()), false);
    d.drain()?;
    assert!(d.app.local_trees[&d.local.id].directories[&empty].is_empty());
    for _ in 0..2 {
        d.app.local_tree_open(&d.local, Some(empty.clone()), false);
        assert!(
            d.app.pending.is_empty(),
            "known empty directory is a cached result"
        );
    }
    Ok(())
}
