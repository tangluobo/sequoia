use super::*;
fn entry(index: usize) -> Entry {
    Entry {
        name: format!("{index}.png"),
        path: format!("/{index}.png"),
        directory: false,
        symlink: false,
        size: 1200,
        modified: 0,
        permissions: 0,
    }
}
fn asset() -> Asset {
    Asset {
        width: 1,
        height: 1,
        picture: tomato_services::markdown::Picture {
            gif: None,
            width: 1,
            height: 1,
            rgba: vec![1, 2, 3, 255],
        },
        gif: None,
    }
}
fn reply(request: &Request) -> Result<Output, String> {
    Ok(Output::Image {
        path: request.path.clone(),
        mode: request.mode,
        asset: asset(),
    })
}
fn items() -> Arc<Vec<Item>> {
    Arc::new(
        (0..3)
            .map(|i| Item {
                name: format!("{i}.png"),
                path: format!("/{i}.png"),
                size: 1200,
            })
            .collect(),
    )
}

#[test]
fn visible_thumbnails_bound_pending_and_cache_and_cancel_on_refresh() {
    let ctx = egui::Context::default();
    let mut cache = Thumbnails::default();
    cache.set_mode(true);
    for i in 0..8 {
        assert!(cache.visible(&entry(i)).is_none());
    }
    assert_eq!(cache.entries.len(), 4);
    let requests = std::mem::take(&mut cache.queued);
    for request in requests {
        cache.finish(&request, reply(&request), &ctx);
    }
    assert!(cache.visible(&entry(0)).is_some());
    for i in 8..300 {
        cache.visible(&entry(i));
        let request = cache.queued.pop().unwrap();
        cache.finish(&request, reply(&request), &ctx);
    }
    assert_eq!(cache.entries.len(), 256);
    cache.visible(&entry(999));
    let cancelled = cache.queued[0].clone();
    cache.suspend();
    assert!(cancelled.cancel.is_cancelled());
    cache.finish(&cancelled, reply(&cancelled), &ctx);
    assert!(cache.entries.is_empty());
    assert!(cache.visible(&entry(0)).is_none());
    assert!(cache.queued.is_empty());
    cache.clear();
    cache.visible(&entry(0));
    assert_eq!(cache.queued.len(), 1);
    cache.set_mode(false);
    assert!(cache.entries.is_empty());
}

#[test]
fn late_image_reply_after_navigation_cannot_replace_current_preview() {
    let ctx = egui::Context::default();
    let mut workspace = Workspace::default();
    let mut preview = Preview::new(items(), 0);
    let old = preview.request.clone();
    preview.select(1);
    assert!(old.cancel.is_cancelled());
    let current = preview.request.clone();
    workspace.previews.push(preview);
    workspace.image_reply(old.clone(), reply(&old), &ctx);
    assert!(workspace.previews[0].texture.is_none());
    workspace.image_reply(current.clone(), reply(&current), &ctx);
    assert!(workspace.previews[0].texture.is_some());
    assert_eq!(
        workspace.previews[0].title,
        "1.png  |  1x1  |  1.2 KB  (2/3)"
    );
    workspace.previews[0].closed = true;
    workspace.queue_images();
    assert!(workspace.previews.is_empty());
    assert!(current.cancel.is_cancelled());
}

#[test]
fn deletion_uses_dispatched_target_even_when_current_image_changed() {
    let mut workspace = Workspace::default();
    let mut preview = Preview::new(items(), 0);
    let removed = preview.items[0].clone();
    let id = preview.id.clone();
    preview.select(2);
    workspace.previews.push(preview);
    workspace.preview_action_reply(
        (id.clone(), removed, PreviewAction::Delete),
        Ok(Output::Done("deleted".into())),
    );
    let preview = &workspace.previews[0];
    assert_eq!(preview.items.len(), 2);
    assert_eq!(preview.items[preview.index].path, "/2.png");
    assert!(workspace.panes[&Section::Files].refresh);
    let target = workspace.previews[0].items[0].clone();
    workspace.preview_action_reply(
        (id, target, PreviewAction::Delete),
        Err("permission denied".into()),
    );
    assert_eq!(workspace.previews[0].items.len(), 2);
    assert_eq!(workspace.previews[0].notice.as_ref().unwrap().0, "删除失败");
}

#[test]
fn source_zoom_uses_both_minimum_dimensions_and_width_based_maximum() {
    let mut preview = Preview::new(items(), 0);
    preview.dimensions = egui::vec2(100., 200.);
    preview.zoom(1.);
    assert!((preview.scale - 1.05).abs() < 0.0001);
    preview.scale = 5.;
    preview.zoom(1.);
    assert_eq!(preview.scale, 5.);
    preview.scale = 0.5;
    preview.zoom(-1.);
    assert_eq!(preview.scale, 0.5);
}
