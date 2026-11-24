use super::*;
use crate::tabs::database_objects::window::{Draft, Mutation};
use tomato_core::{
    database_objects::{Field, Object},
    workbench::TableId,
};

fn workbench() -> Workbench {
    let mut view = Workbench {
        schema: "owned".into(),
        browser: Some(Box::new(Draft::default())),
        ..Default::default()
    };
    let objects = ["first", "second"].map(|name| Object {
        table: TableId {
            schema: "owned".into(),
            name: name.into(),
        },
        view: false,
        comment: String::new(),
        engine: String::new(),
        auto_increment: String::new(),
        update_time: String::new(),
        data_length: String::new(),
        rows: String::new(),
        fields: Vec::new(),
    });
    view.receive_object_catalog(
        Catalog {
            objects: objects.into(),
            foreign_keys: Vec::new(),
        },
        true,
    );
    view
}
fn payload(name: &str, comment: String) -> Vec<Fields> {
    vec![Fields {
        name: name.into(),
        columns: vec![Field {
            name: "field_name".into(),
            comment,
        }],
    }]
}
#[test]
fn ordinary_preload_moves_fields_and_keeps_editor_selection_confirmation_and_header_buffers()
-> Result<()> {
    let mut view = workbench();
    let ctx = egui::Context::default();
    let browser = view.browser.as_mut().unwrap();
    browser.click(0, false, false);
    browser.start_rename(&ctx, 0);
    browser.rename.as_mut().unwrap().text = "editing a name".into();
    browser.confirm(Mutation::Drop);
    browser.widths = [211.; 7];
    browser.widths_pending = false;
    browser.sort = Some((0, true));
    let header = browser.catalog.objects[0].table.name.as_ptr();
    view.receive_object_fields(payload("first", "metadata".into()))?;
    let browser = view.browser.as_ref().unwrap();
    assert_eq!(
        browser.catalog.objects[0].table.name.as_ptr(),
        header,
        "ordinary two-owner flow must not clone headers"
    );
    assert_eq!(browser.rename.as_ref().unwrap().text, "editing a name");
    assert_eq!(browser.selected.iter().copied().collect::<Vec<_>>(), [0]);
    assert_eq!(
        browser.confirmation.as_ref().unwrap().targets[0].table.name,
        "first"
    );
    assert_eq!(browser.widths, [211.; 7]);
    assert!(!browser.widths_pending && !browser.state.fields_pending);
    assert_eq!(browser.sort, Some((0, true)));
    assert_eq!(browser.catalog.objects[0].fields[0].comment, "metadata");
    assert!(Arc::ptr_eq(
        &browser.catalog,
        view.objects.as_ref().unwrap()
    ));
    Ok(())
}
#[test]
fn current_nonempty_search_includes_new_comments_without_consuming_its_debounce() -> Result<()> {
    let mut view = workbench();
    let browser = view.browser.as_mut().unwrap();
    browser.search = "special remark".into();
    browser.search_due = Some(100.);
    browser.filter();
    assert!(browser.filtered.is_empty());
    view.receive_object_fields(payload("second", "special remark".into()))?;
    let browser = view.browser.as_ref().unwrap();
    assert_eq!(browser.filtered, [1]);
    assert_eq!(browser.search_due, Some(100.));
    assert!(browser.selected.is_empty());
    assert!(!browser.state.fields_pending);
    Ok(())
}
#[test]
fn oversized_or_duplicate_payload_never_partially_replaces_the_published_catalog() {
    let mut view = workbench();
    let original = view.objects.as_ref().unwrap().clone();
    assert!(
        view.receive_object_fields(payload(
            "first",
            "x".repeat(tomato_core::limits::QUERY_BYTES)
        ))
        .is_err()
    );
    assert!(Arc::ptr_eq(&original, view.objects.as_ref().unwrap()));
    assert!(
        view.browser.as_ref().unwrap().catalog.objects[0]
            .fields
            .is_empty()
    );
    let mut duplicate = payload("first", "one".into());
    duplicate.extend(payload("first", "two".into()));
    assert!(view.receive_object_fields(duplicate).is_err());
    assert!(Arc::ptr_eq(&original, view.objects.as_ref().unwrap()));
}
