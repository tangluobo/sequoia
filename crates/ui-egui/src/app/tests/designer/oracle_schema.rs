//! Actual tree → objects/search and new query → actor → keyboard completion.
use super::*;
fn browser<'a>(app: &'a TomatoApp, id: &str) -> &'a crate::tabs::database_objects::window::Draft {
    let Panel::Sql { workbench, .. } = &app.tabs.iter().find(|tab| tab.id == id).unwrap().panel
    else {
        unreachable!()
    };
    workbench.browser.as_ref().unwrap()
}
pub(super) fn exercise(
    app: &mut TomatoApp,
    connection: &Connection,
    scope: &str,
    db: &mut tomato_services::Databases,
) -> anyhow::Result<()> {
    let target = format!("\"{scope}\".\"schema_ui\"");
    app.runtime.as_ref().unwrap().block_on(db.query(connection, &format!(
        "CREATE TABLE {target} (\"id\" NUMBER PRIMARY KEY,\"同名 字段\" VARCHAR2(40)); COMMENT ON COLUMN {target}.\"同名 字段\" IS '专用搜索备注'"
    )))?;
    let mut opened = Vec::new();
    let result = (|| -> anyhow::Result<()> {
        sidebar_menu(app, "表", "打开对象")?;
        let id = app
            .active
            .clone()
            .ok_or_else(|| anyhow::anyhow!("tree object entry did not open"))?;
        opened.push(id.clone());
        wait(app)?;
        let object = browser(app, &id)
            .catalog
            .objects
            .iter()
            .find(|object| object.table.name == "schema_ui")
            .ok_or_else(|| anyhow::anyhow!("real object catalog missing schema_ui"))?;
        assert_eq!(object.fields[1].name, "同名 字段");
        assert_eq!(object.fields[1].comment, "专用搜索备注");
        click(app, &id, 760., 16.);
        frame(app, &id, vec![egui::Event::Text("专用搜索备注".into())]);
        let due = browser(app, &id)
            .search_due
            .ok_or_else(|| anyhow::anyhow!("search did not receive keyboard input"))?;
        let ctx = app.context.clone();
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(due + 0.01),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(896., 650.),
                )),
                ..Default::default()
            },
            |_| draw(app, &ctx, &id),
        );
        output.textures_delta.clear();
        let state = browser(app, &id);
        assert_eq!(state.filtered.len(), 1);
        assert_eq!(
            state.catalog.objects[state.filtered[0]].table.name,
            "schema_ui"
        );
        app.new_scoped_query(connection.clone(), scope.into());
        let query = app
            .active
            .clone()
            .ok_or_else(|| anyhow::anyhow!("new query entry did not open"))?;
        opened.push(query.clone());
        wait(app)?;
        frame(app, &query, vec![]);
        frame(
            app,
            &query,
            vec![egui::Event::Text("SELECT schema_u".into())],
        );
        let Panel::Sql { workbench, sql, .. } =
            &app.tabs.iter().find(|tab| tab.id == query).unwrap().panel
        else {
            unreachable!()
        };
        assert_eq!(sql, "SELECT schema_u");
        let completion = &workbench.completion;
        assert!(
            completion.open
                && completion
                    .matches
                    .candidates
                    .iter()
                    .any(|item| item.text == "schema_ui"),
            "real Oracle metadata did not reach keyboard completion"
        );
        let table = completion
            .catalog()
            .tables
            .iter()
            .find(|table| table.name == "schema_ui")
            .unwrap();
        assert_eq!(table.columns, ["id", "同名 字段"]);
        let ai = completion
            .catalog()
            .ai_tables
            .iter()
            .find(|table| table.text.starts_with("TABLE schema_ui\n"))
            .unwrap();
        assert!(ai.text.contains("id NUMBER PRIMARY KEY") && ai.text.contains("专用搜索备注"));
        Ok(())
    })();
    for id in opened {
        app.close_tab(&id, false);
    }
    let cleanup = app
        .runtime
        .as_ref()
        .unwrap()
        .block_on(db.query(connection, &format!("DROP TABLE {target} PURGE")));
    result.and(cleanup.map(|_| ()))
}
