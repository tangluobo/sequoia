use super::*;
use crate::app::tests::native_bridge::Bridge;
#[test]
fn table_failure_opens_native_alert_without_banner_or_losing_rows_or_drafts() -> anyhow::Result<()>
{
    let ctx = egui::Context::default();
    let bridge = Bridge::new(&ctx);
    let mut tab = crate::tabs::create_tab(tomato_core::Connection::new("SQLITE"), None)?;
    let definition: tomato_core::workbench::TableDefinition = serde_json::from_value(
        serde_json::json!({"table":{"schema":"main","name":"fixture"},"columns":[{"name":"value","data_type":"TEXT","nullable":true,"default":null,"primary_key":1,"generated":false,"auto_increment":false,"binary":false,"type_schema":"","type_name":"TEXT"}],"view":false,"transactional":true}),
    )?;
    if let Panel::Sql { workbench, .. } = &mut tab.panel {
        workbench.table = Some(definition.table.clone());
        workbench.definition = Some(definition.clone());
        workbench.page = Some(tomato_core::workbench::TablePage {
            definition,
            rows: vec![vec![tomato_core::workbench::Cell::Text(
                "retained-row".into(),
            )]],
            total: 1,
            offset: 0,
            limit: 100,
            truncated: false,
            query: Default::default(),
            sql: "SELECT value FROM fixture".into(),
        });
        workbench
            .inserted
            .push(vec![Some(tomato_core::workbench::Cell::Text(
                "unsaved-row".into(),
            ))]);
    }
    let mut icons = crate::icons::Icons::default();
    let first = bridge.root_frame(&ctx, vec![], vec![], |ui| {
        crate::panels::render(ui, &mut tab, &mut icons);
    });
    let error = "database fixture error (os error 10053)";
    tab.error = Some(error.into());
    let second = bridge.root_frame(&ctx, vec![], vec![], |ui| {
        crate::panels::render(ui, &mut tab, &mut icons);
    });
    let text_position = |output: &egui::FullOutput| {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.job.text == "retained-row" => Some(t.pos),
            _ => None,
        })
    };
    assert!(text_position(&first).is_some());
    assert_eq!(text_position(&first), text_position(&second));
    assert!(
        !second
            .shapes
            .iter()
            .any(|shape| matches!(&shape.shape,egui::Shape::Text(t) if t.galley.job.text==error))
    );
    assert!(tab.error.is_none());
    bridge.text(error)?;
    let ok = bridge.text("确定")?;
    for pressed in [true, false] {
        bridge.root_frame(
            &ctx,
            vec![],
            vec![
                egui::Event::PointerMoved(ok),
                egui::Event::PointerButton {
                    pos: ok,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            |ui| {
                crate::panels::render(ui, &mut tab, &mut icons);
            },
        );
    }
    if let Panel::Sql { workbench, .. } = &tab.panel {
        assert!(workbench.failure.is_none());
        assert_eq!(
            workbench.page.as_ref().unwrap().sql,
            "SELECT value FROM fixture"
        );
        assert_eq!(workbench.page.as_ref().unwrap().rows.len(), 1);
        assert_eq!(workbench.inserted.len(), 1);
    } else {
        unreachable!()
    }
    Ok(())
}
