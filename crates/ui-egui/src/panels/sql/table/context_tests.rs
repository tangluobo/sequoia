//! Real egui pointer/menu input and owned SQLite commands, never the shared clipboard/config.
use super::*;
fn state() -> Workbench {
    let definition:TableDefinition=serde_json::from_value(serde_json::json!({
        "table":{"schema":"main","name":"items"},"view":false,"transactional":true,
        "comments":{"user_name":"姓名"},
        "columns":(["id","user_name","note"].map(|name|serde_json::json!({"name":name,"data_type":"TEXT","nullable":true,"default":null,"primary_key":if name=="id"{1}else{0},"generated":false,"auto_increment":false,"binary":false,"type_schema":"","type_name":"TEXT"})))
    })).unwrap();
    Workbench {
        table: Some(definition.table.clone()),
        definition: Some(definition.clone()),
        page: Some(TablePage {
            definition,
            rows: vec![
                vec![Cell::Integer(1), Cell::Text("Alice".into()), Cell::Null],
                vec![
                    Cell::Integer(2),
                    Cell::Text("Bob".into()),
                    Cell::Text("note".into()),
                ],
                vec![
                    Cell::Integer(3),
                    Cell::Text("Charlie".into()),
                    Cell::Text("last".into()),
                ],
            ],
            offset: 0,
            limit: 1000,
            total: 3,
            truncated: false,
            query: Default::default(),
            sql: String::new(),
        }),
        table_view: crate::tabs::workbench::TableView {
            widths: vec![80., 100., 110.],
            ..Default::default()
        },
        ..Default::default()
    }
}
fn context() -> egui::Context {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("table-clipboard-available"), true));
    ctx
}
fn input(ctx: &egui::Context, mut events: Vec<egui::Event>) -> egui::RawInput {
    let modifiers = events
        .iter()
        .rev()
        .find_map(|event| match event {
            egui::Event::PointerButton { modifiers, .. }
            | egui::Event::Key { modifiers, .. }
            | egui::Event::ModifiersChanged(modifiers) => Some(*modifiers),
            _ => None,
        })
        .unwrap_or_default();
    if ctx.input(|input| input.modifiers) != modifiers {
        events.insert(0, egui::Event::ModifiersChanged(modifiers));
    }
    egui::RawInput {
        time: Some(ctx.input(|input| input.time) + 0.1),
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000., 800.),
        )),
        events,
        ..Default::default()
    }
}
fn frame(
    ctx: &egui::Context,
    state: &mut Workbench,
    events: Vec<egui::Event>,
    writable: bool,
) -> egui::FullOutput {
    let mut out = ctx.run_ui(input(ctx, events), |ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        grid::grid(ui, state, writable, "SQLITE");
    });
    out.textures_delta.clear();
    out
}
fn pointer(
    point: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
    modifiers: egui::Modifiers,
) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(point),
        egui::Event::PointerButton {
            pos: point,
            button,
            pressed,
            modifiers,
        },
    ]
}
fn click(
    ctx: &egui::Context,
    state: &mut Workbench,
    point: egui::Pos2,
    button: egui::PointerButton,
    modifiers: egui::Modifiers,
    writable: bool,
) -> egui::FullOutput {
    frame(
        ctx,
        state,
        pointer(point, button, true, modifiers),
        writable,
    );
    frame(
        ctx,
        state,
        pointer(point, button, false, modifiers),
        writable,
    )
}
fn point(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == label => {
                Some(text.pos + egui::vec2(5., 5.))
            }
            _ => None,
        })
        .max_by(|a, b| a.x.total_cmp(&b.x))
        .unwrap_or_else(|| {
            panic!(
                "Missing actual control: {label}; painted labels: {:?}",
                output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            )
        })
}
fn open(
    ctx: &egui::Context,
    state: &mut Workbench,
    at: egui::Pos2,
    writable: bool,
) -> egui::FullOutput {
    // Honor the repaint requested when the previous popup closes before the next press.
    frame(ctx, state, vec![], writable);
    click(
        ctx,
        state,
        at,
        egui::PointerButton::Secondary,
        egui::Modifiers::NONE,
        writable,
    );
    frame(ctx, state, vec![], writable)
}
fn choose(
    ctx: &egui::Context,
    state: &mut Workbench,
    label: &str,
    writable: bool,
) -> egui::FullOutput {
    let output = frame(ctx, state, vec![], writable);
    click(
        ctx,
        state,
        point(&output, label),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        writable,
    )
}
#[test]
fn selector_single_multi_and_unselected_right_click_share_the_full_java_menu() {
    let ctx = context();
    let mut state = state();
    frame(&ctx, &mut state, vec![], true);
    click(
        &ctx,
        &mut state,
        egui::pos2(12., 45.),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        true,
    );
    click(
        &ctx,
        &mut state,
        egui::pos2(12., 101.),
        egui::PointerButton::Primary,
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..egui::Modifiers::NONE
        },
        true,
    );
    assert_eq!(
        state.selected.iter().copied().collect::<Vec<_>>(),
        vec![0, 2]
    );
    let output = open(&ctx, &mut state, egui::pos2(12., 45.), true);
    assert_eq!(
        state.selected.iter().copied().collect::<Vec<_>>(),
        vec![0, 2]
    );
    let labels = [
        "设置为空字符串",
        "设置为 NULL",
        "删除 记录",
        "复制",
        "复制为",
        "粘贴",
        "保存数据为...",
        "排序",
        "筛选",
        "移除全部排序及筛选",
        "显示",
        "刷新",
    ];
    let positions: Vec<_> = labels.iter().map(|label| point(&output, label).y).collect();
    assert!(
        positions.windows(2).all(|pair| pair[1] > pair[0]),
        "{positions:?}"
    );
    assert!((positions[1] - positions[0] - 20.).abs() < 0.1);
    choose(&ctx, &mut state, "复制", true);
    let output = open(&ctx, &mut state, egui::pos2(12., 73.), true);
    assert_eq!(state.selected.iter().copied().collect::<Vec<_>>(), vec![1]);
    assert!(state.table_view.row_mode);
    point(&output, "删除 记录");
}
#[test]
fn actual_cell_menu_preserves_multi_selection_and_copy_null_apply_to_selected_cells() {
    let ctx = context();
    let mut state = state();
    frame(&ctx, &mut state, vec![], true);
    click(
        &ctx,
        &mut state,
        egui::pos2(120., 45.),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        true,
    );
    click(
        &ctx,
        &mut state,
        egui::pos2(120., 73.),
        egui::PointerButton::Primary,
        egui::Modifiers {
            ctrl: true,
            command: true,
            ..egui::Modifiers::NONE
        },
        true,
    );
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    assert_eq!(
        state.table_view.cells.iter().collect::<Vec<_>>(),
        vec![(0, 1), (1, 1)]
    );
    let output = choose(&ctx, &mut state, "复制", true);
    assert!(
        output.platform_output.commands.iter().any(
            |command| matches!(command,egui::OutputCommand::CopyText(text)if text=="Alice\nBob")
        )
    );
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    choose(&ctx, &mut state, "设置为 NULL", true);
    assert_eq!(state.edits[&0][1], Cell::Null);
    assert_eq!(state.edits[&1][1], Cell::Null);
    open(&ctx, &mut state, egui::pos2(230., 101.), true);
    assert_eq!(
        state.table_view.cells.iter().collect::<Vec<_>>(),
        vec![(2, 2)]
    );
    assert!(!state.table_view.row_mode);
}
#[test]
fn actual_menu_paste_inserts_below_selector_but_overwrites_cells_and_limits_atomically() {
    let ctx = context();
    let mut state = state();
    frame(&ctx, &mut state, vec![], true);
    open(&ctx, &mut state, egui::pos2(12., 45.), true);
    choose(&ctx, &mut state, "粘贴", true);
    assert!(state.table_view.pending_paste);
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Paste(
            "4\tnew\trow\r\n5\tmore\tdata\r\n".into(),
        )],
        true,
    );
    assert_eq!(projection::order(&state), vec![0, 3, 4, 1, 2]);
    assert_eq!(state.inserted.len(), 2);
    assert_eq!(projection::text(&state, 3, 1).as_deref(), Some("new"));
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    choose(&ctx, &mut state, "粘贴", true);
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Paste("edited\ttext".into())],
        true,
    );
    assert_eq!(state.edits[&0][1], Cell::Text("edited".into()));
    assert_eq!(state.inserted.len(), 2);
    let old = state.edits.clone();
    let count = state.inserted.len();
    state.table_view.row_mode = true;
    assert!(context_actions::paste(&mut state, &"x\n".repeat(1001)).is_err());
    assert_eq!(state.edits, old);
    assert_eq!(state.inserted.len(), count);
}
#[test]
fn real_nested_copy_sort_filter_requests_display_and_read_only_actions_match_java() {
    let ctx = context();
    let mut state = state();
    frame(&ctx, &mut state, vec![], true);
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    choose(&ctx, &mut state, "复制为", true);
    let output = frame(&ctx, &mut state, vec![], true);
    point(&output, "Update 语句");
    choose(&ctx, &mut state, "JSON 格式", true);
    let output = choose(&ctx, &mut state, "字段转驼峰", true);
    assert!(output.platform_output.commands.iter().any(|command|matches!(command,egui::OutputCommand::CopyText(text)if text.contains("userName")&&text.contains("Alice"))));
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    choose(&ctx, &mut state, "排序", true);
    choose(&ctx, &mut state, "降序排序", true);
    assert_eq!(projection::rows(&state), vec![0, 1, 2]);
    assert!(state.pending_page.as_ref().unwrap().query.sorts[0].descending);
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    choose(&ctx, &mut state, "筛选", true);
    choose(&ctx, &mut state, "字段 = 值", true);
    assert_eq!(projection::rows(&state), vec![0, 1, 2]);
    assert_eq!(state.pending_page.as_ref().unwrap().query.filters.len(), 1);
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    choose(&ctx, &mut state, "移除全部排序及筛选", true);
    assert_eq!(projection::rows(&state), vec![0, 1, 2]);
    let id = egui::Id::new("test-grid");
    context_actions::apply(
        &ctx,
        &mut state,
        context_menu::Action::Visible(1, false),
        id,
        "SQLITE",
        true,
    );
    let out = frame(&ctx, &mut state, vec![], true);
    assert!(!out.shapes.iter().any(
        |shape| matches!(&shape.shape,egui::Shape::Text(text)if text.galley.job.text=="user_name")
    ));
    open(&ctx, &mut state, egui::pos2(120., 45.), false);
    choose(&ctx, &mut state, "设置为 NULL", false);
    assert!(!state.dirty());
    assert!(!state.confirm_delete);
}
#[test]
fn selected_formats_keep_predicates_comments_edits_and_null_separate() {
    let mut state = state();
    for column in 0..3 {
        state.table_view.cells.insert((0, column));
    }
    state.selected.insert(0);
    assert!(
        transfer::formatted(&state, context_menu::Copy::Update, false)
            .unwrap()
            .contains("WHERE id = '1'")
    );
    assert!(
        transfer::formatted(&state, context_menu::Copy::Comments, false)
            .unwrap()
            .contains("姓名：Alice")
    );
    let json = transfer::formatted(&state, context_menu::Copy::Json(false), false).unwrap();
    assert!(json.contains("\"note\": null"));
    state.edits.insert(
        0,
        vec![
            Cell::Integer(1),
            Cell::Text("draft".into()),
            Cell::Text("NULL".into()),
        ],
    );
    assert!(
        transfer::formatted(&state, context_menu::Copy::Json(false), false)
            .unwrap()
            .contains("\"note\": \"NULL\"")
    );
}

fn tab_frame(
    ctx: &egui::Context,
    tab: &mut Tab,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<Intent>) {
    let mut intent = None;
    let mut out = ctx.run_ui(input(ctx, events), |ui| {
        intent = render(ui, tab);
    });
    out.textures_delta.clear();
    (out, intent)
}
fn tab_click(
    ctx: &egui::Context,
    tab: &mut Tab,
    at: egui::Pos2,
    button: egui::PointerButton,
) -> (egui::FullOutput, Option<Intent>) {
    tab_frame(ctx, tab, pointer(at, button, true, egui::Modifiers::NONE));
    tab_frame(ctx, tab, pointer(at, button, false, egui::Modifiers::NONE))
}
fn tab_choose(
    ctx: &egui::Context,
    tab: &mut Tab,
    label: &str,
) -> (egui::FullOutput, Option<Intent>) {
    let (out, _) = tab_frame(ctx, tab, vec![]);
    tab_click(ctx, tab, point(&out, label), egui::PointerButton::Primary)
}
fn workbench(tab: &mut Tab) -> &mut Workbench {
    let Panel::Sql { workbench, .. } = &mut tab.panel else {
        unreachable!()
    };
    workbench
}
#[test]
fn real_menu_paste_save_immediate_delete_preserves_other_drafts_and_opens_export_wizard()
-> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let directory=tempfile::tempdir()?;
        let mut c=tomato_core::Connection::new("SQLITE");c.set("database",directory.path().join("owned.sqlite").to_string_lossy().as_ref());
        let mut db=tomato_services::database::Databases::default();
        db.query(&c,"CREATE TABLE items(id INTEGER PRIMARY KEY, user_name TEXT, note TEXT); INSERT INTO items VALUES(1,'Alice',NULL),(2,'Bob','note'),(3,'Charlie','last')").await?;
        let request=PageRequest{table:TableId{schema:"main".into(),name:"items".into()},offset:0,limit:1000,sort:None,query:Default::default()};
        let output=db.workbench(&c,DatabaseAction::Page{request:request.clone()}).await?;
        let mut state=Workbench{table:Some(request.table.clone()),..Default::default()};state.receive(output);state.table_view.widths=vec![80.,100.,110.];
        let mut tab=Tab{id:"owned-menu".into(),source_id:c.id.clone(),title:"items".into(),panel:Panel::Sql{connection:c.clone(),sql:String::new(),results:Vec::new(),selected:0,workbench:Box::new(state)},busy:false,error:None};
        let ctx=context();let (out,_)=tab_frame(&ctx,&mut tab,vec![]);
        tab_click(&ctx,&mut tab,point(&out,"Alice"),egui::PointerButton::Secondary);tab_choose(&ctx,&mut tab,"粘贴");
        tab_frame(&ctx,&mut tab,vec![egui::Event::Paste("changed".into())]);
        let save=ctx.data(|data|data.get_temp::<egui::Rect>(egui::Id::new(("table-toolbar","保存")))).unwrap().center();
        let (_,intent)=tab_click(&ctx,&mut tab,save,egui::PointerButton::Primary);
        let Some(Intent::Command(command))=intent else{anyhow::bail!("Save toolbar did not submit a command")};
        let Command::Database{action,..}=*command else{anyhow::bail!("wrong save command")};
        workbench(&mut tab).receive(db.workbench(&c,action).await?);
        assert_eq!(db.query(&c,"SELECT user_name FROM items WHERE id=1").await?[0].rows[0][0],"changed");
        let (out,_)=tab_frame(&ctx,&mut tab,vec![]);
        tab_click(&ctx,&mut tab,point(&out,"Bob"),egui::PointerButton::Secondary);tab_choose(&ctx,&mut tab,"粘贴");
        tab_frame(&ctx,&mut tab,vec![egui::Event::Paste("unsaved".into())]);
        let (out,_)=tab_frame(&ctx,&mut tab,vec![]);
        tab_click(&ctx,&mut tab,point(&out,"changed"),egui::PointerButton::Secondary);tab_choose(&ctx,&mut tab,"删除 记录");
        let (_,intent)=tab_choose(&ctx,&mut tab,"确定");
        let Some(Intent::Command(command))=intent else{anyhow::bail!("Delete confirmation did not submit a command")};
        let Command::Database{action,..}=*command else{anyhow::bail!("wrong delete command")};
        assert!(matches!(&action,DatabaseAction::DeleteRows{..}));
        workbench(&mut tab).receive(db.workbench(&c,action).await?);
        assert_eq!(db.query(&c,"SELECT COUNT(*) FROM items").await?[0].rows[0][0],"2");
        assert_eq!(workbench(&mut tab).edits[&0][1],Cell::Text("unsaved".into()));
        assert_eq!(db.query(&c,"SELECT user_name FROM items WHERE id=2").await?[0].rows[0][0],"Bob");
        let (out,_)=tab_frame(&ctx,&mut tab,vec![]);tab_click(&ctx,&mut tab,point(&out,"unsaved"),egui::PointerButton::Secondary);
        let (_,intent)=tab_choose(&ctx,&mut tab,"保存数据为...");
        assert!(matches!(intent,Some(Intent::ExportData(None))));
        let snapshot=export_snapshot(&ctx,workbench(&mut tab),"SQLITE")?;
        assert!(snapshot.rows.iter().any(|row|row.contains(&Cell::Text("unsaved".into()))));
        assert!(snapshot.rows.iter().any(|row|row.contains(&Cell::Text("Charlie".into()))));
        assert!(!snapshot.rows.iter().any(|row|row.contains(&Cell::Text("changed".into()))));
        Ok(())
    })
}

#[test]
fn selector_ctrl_clears_a_partial_row_and_mixed_cell_selection_retains_insert_paste_mode() {
    let ctx = context();
    let mut state = state();
    frame(&ctx, &mut state, vec![], true);
    click(
        &ctx,
        &mut state,
        egui::pos2(120., 45.),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        true,
    );
    let ctrl = egui::Modifiers {
        ctrl: true,
        command: true,
        ..egui::Modifiers::NONE
    };
    click(
        &ctx,
        &mut state,
        egui::pos2(12., 45.),
        egui::PointerButton::Primary,
        ctrl,
        true,
    );
    assert!(state.selected.is_empty());
    assert!(state.table_view.cells.is_empty());
    click(
        &ctx,
        &mut state,
        egui::pos2(12., 45.),
        egui::PointerButton::Primary,
        egui::Modifiers::NONE,
        true,
    );
    click(
        &ctx,
        &mut state,
        egui::pos2(120., 101.),
        egui::PointerButton::Primary,
        ctrl,
        true,
    );
    assert!(state.table_view.row_mode);
    assert!(!state.table_view.selector_focus);
    assert!(state.table_view.selector_rows.contains(&0));
    context_actions::paste(&mut state, "4\tinserted\tmixed").unwrap();
    assert_eq!(state.inserted.len(), 1);
    assert!(!state.edits.contains_key(&2));
}

#[test]
fn empty_page_body_has_refresh_export_and_paste_but_header_filler_has_no_data_menu() {
    let ctx = context();
    let mut state = state();
    state.page.as_mut().unwrap().rows.clear();
    state.page.as_mut().unwrap().total = 0;
    frame(&ctx, &mut state, vec![], true);
    let output = open(&ctx, &mut state, egui::pos2(45., 80.), true);
    point(&output, "刷新");
    point(&output, "保存数据为...");
    choose(&ctx, &mut state, "粘贴", true);
    frame(
        &ctx,
        &mut state,
        vec![egui::Event::Paste("7\tfirst\tnew".into())],
        true,
    );
    assert_eq!(state.inserted.len(), 1);
    frame(&ctx, &mut state, vec![], true);
    let output = open(&ctx, &mut state, egui::pos2(450., 20.), true);
    assert!(!output.shapes.iter().any(|shape|matches!(&shape.shape,egui::Shape::Text(text)if text.galley.job.text=="保存数据为...")));
}

#[test]
fn unapplied_server_filters_do_not_hide_or_reassign_current_selections() {
    let ctx = context();
    let mut state = state();
    frame(&ctx, &mut state, vec![], true);
    open(&ctx, &mut state, egui::pos2(120., 45.), true);
    choose(&ctx, &mut state, "筛选", true);
    choose(&ctx, &mut state, "字段 != 值", true);
    frame(&ctx, &mut state, vec![], true);
    assert_eq!(projection::rows(&state), vec![0, 1, 2]);
    assert_eq!(state.pending_page.as_ref().unwrap().query.filters.len(), 1);
    state.search = "Charlie".into();
    frame(&ctx, &mut state, vec![], true);
    assert_eq!(projection::rows(&state), vec![0, 1, 2]);
    assert!(state.query.search.is_empty());
}

#[test]
fn table_menu_filters_and_multisort_dispatch_real_database_queries() -> anyhow::Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let directory = tempfile::tempdir()?;
        let mut connection = tomato_core::Connection::new("SQLITE");
        connection.set("database", directory.path().join("owned-filter.sqlite").to_string_lossy().as_ref());
        let mut db = tomato_services::database::Databases::default();
        db.query(&connection, "CREATE TABLE items(id INTEGER PRIMARY KEY,user_name TEXT,note TEXT);WITH RECURSIVE n(x) AS(SELECT 0 UNION ALL SELECT x+1 FROM n WHERE x<2204)INSERT INTO items SELECT x,'row '||x,CASE WHEN x%2=0 THEN 'even' ELSE 'odd' END FROM n").await?;
        let request = PageRequest { table: TableId { schema: "main".into(), name: "items".into() }, offset: 0, limit: 1000, sort: None, query: Default::default() };
        let mut initial = Workbench { table: Some(request.table.clone()), ..Default::default() };
        initial.receive(db.workbench(&connection, DatabaseAction::Page { request }).await?);
        initial.table_view.widths = vec![80., 100., 110.];
        let mut tab = Tab { id: "owned-filter".into(), source_id: connection.id.clone(), title: "items".into(), panel: Panel::Sql { connection: connection.clone(), sql: String::new(), results: Vec::new(), selected: 0, workbench: Box::new(initial) }, busy: false, error: None };
        let ctx = context();
        let (out, _) = tab_frame(&ctx, &mut tab, vec![]);
        tab_click(&ctx, &mut tab, point(&out, "0"), egui::PointerButton::Secondary);
        let (out, _) = tab_frame(&ctx, &mut tab, vec![]);
        let menu_sort = out.shapes.iter().rev().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                (text.galley.job.text == "排序").then_some(text.pos + egui::vec2(4., 5.))
            } else { None }
        }).expect("actual context menu sort, after the toolbar label");
        tab_click(&ctx, &mut tab, menu_sort, egui::PointerButton::Primary);
        let (_, intent) = tab_choose(&ctx, &mut tab, "降序排序");
        let Some(Intent::Command(command)) = intent else { anyhow::bail!("Sort did not dispatch") };
        let Command::Database { action, .. } = *command else { anyhow::bail!("Sort not a database action") };
        workbench(&mut tab).receive(db.workbench(&connection, action).await?);
        assert_eq!(workbench(&mut tab).page.as_ref().unwrap().rows[0][0].display(), "2204");
        tab_frame(&ctx, &mut tab, vec![]);
        tab_frame(&ctx, &mut tab, vec![
            egui::Event::PointerMoved(egui::pos2(150., 200.)),
            egui::Event::MouseWheel { phase: egui::TouchPhase::Move, unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0., -2000.), modifiers: egui::Modifiers::NONE },
        ]);
        for _ in 0..5 { tab_frame(&ctx, &mut tab, vec![]); }
        let (scrolled, _) = tab_frame(&ctx, &mut tab, vec![]);
        assert!(!scrolled.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "2204")), "Wheel fixture must leave the first row before applying criteria");
        let old_sql = workbench(&mut tab).page.as_ref().unwrap().sql.clone();
        // Open the real toolbar builder and apply a draft condition, rather than
        // changing the current page projection or executing a separate test SQL.
        workbench(&mut tab).table_view.query_draft = Some(TableQuery {
            filters: vec![ColumnFilter { column: "id".into(), condition: tomato_core::result_filter::Condition { operator: tomato_core::result_filter::Operator::Greater, expected: Some("1999".into()) } }],
            sorts: vec![Sort { column: "note".into(), descending: false }, Sort { column: "id".into(), descending: true }],
            ..Default::default()
        });
        tab_choose(&ctx, &mut tab, "筛选");
        assert_eq!(workbench(&mut tab).page.as_ref().unwrap().sql, old_sql, "Draft must not change executed SQL");
        let (_, intent) = tab_choose(&ctx, &mut tab, "应用筛选和排序");
        let Some(Intent::Command(command)) = intent else { anyhow::bail!("Filter builder did not dispatch") };
        let Command::Database { action, .. } = *command else { anyhow::bail!("Filter not a database action") };
        workbench(&mut tab).receive(db.workbench(&connection, action).await?);
        let page = workbench(&mut tab).page.as_ref().unwrap();
        assert_eq!(page.total, 205);
        assert!(page.sql.contains("WHERE (\"id\" > 1999)"));
        assert!(page.sql.contains("ORDER BY \"note\" ASC, \"id\" DESC"));
        assert_eq!(page.rows[0][0].display(), "2204");
        assert_eq!(page.rows[102][0].display(), "2000");
        assert_eq!(page.rows[103][0].display(), "2203");
        assert!(workbench(&mut tab).selected.is_empty());
        let (out, _) = tab_frame(&ctx, &mut tab, vec![]);
        assert!(out.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text == "2204")), "New criteria must reset the visible viewport to its first row");
        // Cancelling the existing discard dialog keeps edits, active criteria,
        // loaded rows and captured executed SQL together.
        let state = workbench(&mut tab);
        let old_sql = state.page.as_ref().unwrap().sql.clone();
        state.edits.insert(0, vec![Cell::Integer(2204), Cell::Text("draft".into()), Cell::Text("even".into())]);
        state.queue_query(TableQuery::default());
        let (_, intent) = tab_frame(&ctx, &mut tab, vec![]);
        assert!(intent.is_none());
        let (_, intent) = tab_choose(&ctx, &mut tab, "取消");
        assert!(intent.is_none());
        let state = workbench(&mut tab);
        assert!(state.dirty() && state.pending_page.is_none());
        assert_eq!(state.page.as_ref().unwrap().sql, old_sql);
        assert_eq!(state.query.filters.len(), 1);
        Ok(())
    })
}
