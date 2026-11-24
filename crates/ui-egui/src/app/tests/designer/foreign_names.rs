//! Existing field labels → real controls → executable SQL → database/data-tab readback.
use super::auxiliary::{fill, labels};
use super::*;
use tomato_core::{
    table_design::auxiliary::fields_label,
    workbench::{TableId, TableInfo},
};

pub(super) fn output(app: &mut TomatoApp, id: &str, events: Vec<egui::Event>) -> egui::FullOutput {
    let ctx = app.context.clone();
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(896., 650.),
            )),
            events,
            ..Default::default()
        },
        |_| draw(app, &ctx, id),
    );
    output.textures_delta.clear();
    output
}
fn choice(app: &mut TomatoApp, id: &str, label: &str, value: &str) -> anyhow::Result<()> {
    let point = labels(app, id)
        .into_iter()
        .filter(|(text, _)| text == label)
        .max_by(|a, b| a.1.y.total_cmp(&b.1.y))
        .map(|(_, point)| point)
        .ok_or_else(|| anyhow::anyhow!("missing choice label {label}"))?;
    let paint = output(app, id, vec![]);
    let arrow = paint
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Path(path) if path.closed && path.points.len() == 3 => {
                let center = egui::Rect::from_points(&path.points).center();
                (center.x > point.x + 100. && center.y >= point.y && center.y < point.y + 26.)
                    .then_some(center)
            }
            _ => None,
        })
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing dropdown arrow for {label}"))?;
    click(app, id, arrow.x, arrow.y);
    let point = labels(app, id)
        .into_iter()
        .rev()
        .find(|(text, _)| text == value)
        .map(|(_, point)| point)
        .ok_or_else(|| anyhow::anyhow!("missing choice {value}"))?;
    click(app, id, point.x + 8., point.y + 6.);
    Ok(())
}
pub(super) fn exercise(
    app: &mut TomatoApp,
    c: &Connection,
    scope: &str,
    db: &mut tomato_services::Databases,
) -> anyhow::Result<()> {
    let dialect = tomato_services::dialect::resolve(c.kind())?;
    let q = |s: &str| dialect.quote(s);
    let locals = ["中文,字段", "a\"b", "a`b", " leading"]
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let mut name = format!("{s}_{i}_");
            name.push_str(&"x".repeat(63 - name.len()));
            name
        })
        .collect::<Vec<_>>();
    let references = locals.iter().map(|s| format!("p{s}")).collect::<Vec<_>>();
    let parent_order = references.iter().rev().cloned().collect::<Vec<_>>();
    let columns = |names: &[String]| -> anyhow::Result<String> {
        Ok(names
            .iter()
            .map(|name| Ok(format!("{} INTEGER", q(name)?)))
            .collect::<anyhow::Result<Vec<_>>>()?
            .join(","))
    };
    let list = |names: &[String]| -> anyhow::Result<String> {
        Ok(names
            .iter()
            .map(|name| q(name))
            .collect::<anyhow::Result<Vec<_>>>()?
            .join(","))
    };
    let child_info = TableInfo {
        table: TableId {
            schema: scope.into(),
            name: "ui_names_child".into(),
        },
        view: false,
    };
    let child = format!("{}.{}", q(scope)?, q(&child_info.table.name)?);
    let parent = format!("{}.{}", q(scope)?, q("ui_names_parent")?);
    // PostgreSQL's identifier limit is in bytes; keep the parent names at 63.
    let references = references
        .into_iter()
        .map(|mut s| {
            s.pop();
            s
        })
        .collect::<Vec<_>>();
    let parent_order = parent_order
        .into_iter()
        .map(|mut s| {
            s.pop();
            s
        })
        .collect::<Vec<_>>();
    let setup = format!(
        "CREATE TABLE {parent} ({},PRIMARY KEY({})); CREATE TABLE {child} ({},CONSTRAINT {} FOREIGN KEY({}) REFERENCES {parent}({}) ON DELETE CASCADE ON UPDATE CASCADE); INSERT INTO {parent} VALUES(1,2,3,4); INSERT INTO {child} VALUES(4,3,2,1)",
        columns(&references)?,
        list(&parent_order)?,
        columns(&locals)?,
        q("fk_ui_names")?,
        list(&locals)?,
        list(&parent_order)?
    );
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(db.query(c, &setup))?;
    if c.kind() == "MYSQL" {
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(db.query(c, "SET SESSION group_concat_max_len=16"))?;
    }
    let host = app.sql_hosts.get_mut(&c.id).unwrap();
    let mut tables = host.tables[scope].as_ref().clone();
    tables.push(child_info.clone());
    host.tables
        .insert(scope.into(), std::sync::Arc::new(tables));
    sidebar_menu(app, &child_info.table.name, "打开表")?;
    let sibling = app.active.clone().unwrap();
    wait(app)?;
    sidebar_menu(app, &child_info.table.name, "设计表")?;
    let id = app.active.clone().unwrap();
    wait(app)?;
    click(app, &id, 120., 45.);
    wait(app)?;
    let before = draft(app, &id).saved.as_ref().unwrap().foreign_keys.rows[0].clone();
    assert_eq!(before[1], fields_label(&locals));
    assert_eq!(before[4], fields_label(&parent_order));
    click(app, &id, 40., 140.);
    let paint = output(app, &id, vec![]);
    let shown = paint
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == before[1] => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        !shown.is_empty() && shown.iter().all(|text| text.galley.rows.len() == 1),
        "long existing field labels must scroll on one line"
    );
    click(app, &id, 130., 76.);
    click(app, &id, 40., 76.);
    for (label, value) in [
        ("名称", "fk_ui_names_rebuilt"),
        ("字段", before[1].as_str()),
        ("参考表", before[3].as_str()),
        ("参考字段", before[4].as_str()),
    ] {
        fill(app, &id, label, value)?;
    }
    choice(app, &id, "删除时", "CASCADE")?;
    choice(app, &id, "更新时", "CASCADE")?;
    click(app, &id, 340., 45.);
    let preview = draft(app, &id).preview.clone();
    assert!(
        preview.contains("DROP") && preview.contains("ADD CONSTRAINT"),
        "{preview}"
    );
    for name in locals.iter().chain(&parent_order) {
        assert!(
            preview.contains(&q(name)?),
            "preview lost a complete identifier"
        );
    }
    click(app, &id, 225., 105.);
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    output(
        app,
        &id,
        vec![
            egui::Event::ModifiersChanged(modifiers),
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
        ],
    );
    let copied = output(app, &id, vec![egui::Event::Copy]);
    output(
        app,
        &id,
        vec![egui::Event::ModifiersChanged(egui::Modifiers::NONE)],
    );
    assert!(
        copied
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command,egui::OutputCommand::CopyText(text) if text==&preview)),
        "SQL preview must copy every complete field name"
    );
    app.runtime.as_ref().unwrap().block_on(db.query(
        c,
        &format!("UPDATE {parent} SET {}=111", q(&parent_order[0])?),
    ))?;
    click(app, &id, 38., 16.);
    wait(app)?;
    assert!(
        !draft(app, &id).dirty,
        "{:?}",
        draft(app, &id).notice.as_ref().map(|n| &n.text)
    );
    assert_eq!(
        draft(app, &id).saved.as_ref().unwrap().foreign_keys.rows[0][0],
        "fk_ui_names_rebuilt"
    );
    let Panel::Sql { workbench, .. } =
        &app.tabs.iter().find(|tab| tab.id == sibling).unwrap().panel
    else {
        unreachable!()
    };
    let page = workbench.page.as_ref().unwrap();
    assert_eq!(page.rows[0][0].display(), "111");
    assert!(
        app.runtime
            .as_ref()
            .unwrap()
            .block_on(db.query(c, &format!("INSERT INTO {child} VALUES(999,999,999,999)")))
            .is_err()
    );
    app.runtime
        .as_ref()
        .unwrap()
        .block_on(db.query(c, &format!("DELETE FROM {parent}")))?;
    click(app, &id, 120., 45.);
    wait(app)?;
    click(app, &id, 40., 140.);
    click(app, &id, 130., 76.);
    click(app, &id, 38., 16.);
    wait(app)?;
    assert!(!draft(app, &id).dirty);
    assert!(
        draft(app, &id)
            .saved
            .as_ref()
            .unwrap()
            .foreign_keys
            .rows
            .is_empty()
    );
    let Panel::Sql { workbench, .. } =
        &app.tabs.iter().find(|tab| tab.id == sibling).unwrap().panel
    else {
        unreachable!()
    };
    assert_eq!(workbench.page.as_ref().unwrap().total, 0);
    app.runtime.as_ref().unwrap().block_on(db.query(
        c,
        &format!(
            "INSERT INTO {child} VALUES(999,999,999,999); DROP TABLE {child}; DROP TABLE {parent}"
        ),
    ))?;
    app.close(&id);
    app.close(&sibling);
    let host = app.sql_hosts.get_mut(&c.id).unwrap();
    let tables = host.tables[scope]
        .iter()
        .filter(|table| {
            table.table.name != "ui_names_child" && table.table.name != "ui_names_parent"
        })
        .cloned()
        .collect();
    host.tables
        .insert(scope.into(), std::sync::Arc::new(tables));
    Ok(())
}
