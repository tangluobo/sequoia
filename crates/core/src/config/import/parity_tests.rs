use super::*;

#[test]
fn many_manual_duplicates_use_first_free_names_and_can_cancel_during_preparation() -> Result<()> {
    let store = Store {
        connections: crate::legacy::decode(
            r#"[{"id":"a","name":"same"},{"id":"b","name":"same (1)"},{"id":"c","name":"same (3)"}]"#,
        )?,
        ..Default::default()
    };
    let nodes: Vec<_> = (0..5000)
        .map(|i| serde_json::json!({"id":format!("new-{i}"),"name":"same"}))
        .collect();
    let plan = ImportPlan::parse(&serde_json::to_string(&nodes)?, &store)?;
    let selected = (0..nodes.len()).collect();
    let added = plan.prepare(&store, &selected)?;
    assert_eq!(added[0].name, "same (2)");
    assert_eq!(added[1].name, "same (4)");
    assert_eq!(added.last().unwrap().name, "same (5002)");
    let checks = std::cell::Cell::new(0);
    assert!(
        plan.prepare_checked(&store, &selected, || {
            checks.set(checks.get() + 1);
            anyhow::ensure!(checks.get() < 3, "cancelled");
            Ok(())
        })
        .is_err()
    );
    assert_eq!(store.connections.len(), 3);
    Ok(())
}

#[test]
fn duplicate_and_null_ids_remain_independent_and_references_use_first_id() -> Result<()> {
    let input = r#"[{"id":"same","name":"first"},{"id":"same","name":"second"},{"id":null,"name":null,"type":"SSH"},{"name":"blank"},{"id":"child","name":"child","type":"MYSQL","parentId":"same","sshTunnelHostId":"same","password":"fixture-password","future":{"x":42}}]"#;
    let mut store = Store::default();
    let plan = ImportPlan::parse(input, &store)?;
    assert_eq!(
        plan.rows().iter().map(|r| r.index).collect::<Vec<_>>(),
        [0, 4, 1, 2, 3]
    );
    plan.apply(&mut store, &BTreeSet::from([0, 1, 2, 3, 4]))?;
    assert_eq!(store.connections.len(), 5);
    assert_ne!(store.connections[0].id, store.connections[1].id);
    assert_eq!(store.connections[2].name, "未命名连接");
    assert_eq!(
        store.connections[4].parent_id.as_deref(),
        Some(store.connections[0].id.as_str())
    );
    assert_eq!(
        store.connections[4].text("sshTunnelHostId"),
        store.connections[0].id
    );
    assert_eq!(store.connections[4].text("password"), "fixture-password");
    assert_eq!(store.connections[4].options["future"]["x"], 42);
    let mut subset = Store::default();
    ImportPlan::parse(input, &subset)?.apply(&mut subset, &BTreeSet::from([1, 4]))?;
    assert!(
        subset.connections[1].parent_id.is_none(),
        "unselected first ID must not bind to the second identical ID"
    );
    assert!(subset.connections[1].text("sshTunnelHostId").is_empty());
    assert!(
        crate::legacy::decode(input).is_err(),
        "normal repository decoding stays strict"
    );
    Ok(())
}

#[test]
fn external_parents_are_visible_and_stale_baselines_never_mutate_the_store() -> Result<()> {
    let mut store = Store {
        connections: crate::legacy::decode(
            r#"[{"id":"external","name":"group"},{"id":"jump","name":"ssh","type":"SSH"}]"#,
        )?,
        ..Default::default()
    };
    let input = r#"[{"id":"new","name":"db","type":"MYSQL","parentId":"external","sshTunnelHostId":"jump"}]"#;
    let mut plan = ImportPlan::parse(input, &store)?;
    assert_eq!((plan.rows()[0].index, plan.rows()[0].depth), (0, 0));
    store.connections.swap(0, 1);
    let before = serde_json::to_vec(&store)?;
    assert!(plan.apply(&mut store, &BTreeSet::from([0])).is_err());
    assert_eq!(serde_json::to_vec(&store)?, before);
    store.connections.swap(0, 1);
    plan.connections[0].name = "changed".into();
    assert!(plan.apply(&mut store, &BTreeSet::from([0])).is_err());
    assert_eq!(store.connections.len(), 2);
    Ok(())
}

#[test]
fn very_deep_import_tree_and_validation_are_iterative() -> Result<()> {
    let nodes:Vec<_>=(0..5000).map(|i|serde_json::json!({"id":format!("n-{i}"),"name":format!("name-{i}"),"parentId":(i>0).then(||format!("n-{}",i-1))})).collect();
    let mut store = Store::default();
    let plan = ImportPlan::parse(&serde_json::to_string(&nodes)?, &store)?;
    assert_eq!(plan.rows().last().unwrap().depth, 4999);
    plan.apply(&mut store, &(0..nodes.len()).collect())?;
    super::super::validate(&store.connections)?;
    Ok(())
}
