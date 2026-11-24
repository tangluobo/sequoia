//! Real catalogs and DDL, including atomic failure, stale snapshots, and opaque preservation.
use super::designer_tests::{edit, load, save};
use super::*;
use tomato_core::table_design::{
    Column, Create,
    indexes::{Field, Index, Kind},
};
pub(super) async fn run(
    db: &mut Databases,
    connection: &Connection,
    kind: &str,
    scope: &str,
) -> Result<()> {
    let mut c = connection.clone();
    if kind == "MYSQL" {
        c.set("database", scope);
    }
    let dialect = tomato_services::dialect::resolve(kind)?;
    let table = TableId {
        schema: scope.into(),
        name: "index_designer".into(),
    };
    let qualified = format!("{}.{}", dialect.quote(scope)?, dialect.quote(&table.name)?);
    let index = Index {
        name: "ix_label".into(),
        method: if kind == "MYSQL" { "BTREE" } else { "btree" }.into(),
        fields: vec![Field {
            name: "label".into(),
            prefix: if kind == "MYSQL" { Some(20) } else { None },
            descending: true,
        }],
        ..Default::default()
    };
    let request = Create {
        auxiliary: Vec::new(),
        table: table.clone(),
        indexes: vec![index],
        columns: vec![
            Column {
                name: "id".into(),
                data_type: "INTEGER".into(),
                primary_key: true,
                not_null: true,
                ..Default::default()
            },
            Column {
                name: "label".into(),
                data_type: "VARCHAR".into(),
                length: "100".into(),
                ..Default::default()
            },
        ],
        options: Default::default(),
        comment: String::new(),
    };
    db.workbench(&c, DatabaseAction::CreateTable { request })
        .await?;
    db.query(
        &c,
        &format!("INSERT INTO {qualified} VALUES(1,'duplicate'),(2,'duplicate')"),
    )
    .await?;
    let snapshot = load(db, &c, &table).await?;
    assert_eq!(snapshot.definition.indexes.len(), 1);
    assert!(
        snapshot.definition.indexes[0].opaque.is_empty(),
        "{:?}",
        snapshot.definition.indexes
    );
    assert!(snapshot.definition.indexes[0].fields[0].descending);
    assert_eq!(
        snapshot.indexes.headers,
        ["名称", "字段", "类型", "方法", "注释"]
    );
    let mut changes = edit(snapshot.clone());
    changes.after.indexes[0].name = "ix_重命名".into();
    let snapshot = save(db, &c, changes).await?;
    assert_eq!(snapshot.definition.indexes[0].name, "ix_重命名");
    let mut invalid = edit(snapshot.clone());
    invalid.after.indexes[0].kind = Kind::Unique;
    assert!(save(db, &c, invalid).await.is_err());
    let intact = load(db, &c, &table).await?;
    assert_eq!(
        intact.definition.indexes, snapshot.definition.indexes,
        "failed UNIQUE must not drop the original index"
    );
    let mut changes = edit(intact);
    changes.after.indexes[0].kind = Kind::Unique;
    changes.after.indexes[0].fields.insert(
        0,
        Field {
            name: "id".into(),
            ..Default::default()
        },
    );
    changes.after.indexes[0].comment = "中文 '注释' \\path".into();
    let snapshot = save(db, &c, changes).await?;
    assert_eq!(snapshot.definition.indexes[0].kind, Kind::Unique);
    assert_eq!(snapshot.definition.indexes[0].fields.len(), 2);
    assert_eq!(snapshot.definition.indexes[0].comment, "中文 '注释' \\path");
    let mut changes = edit(snapshot);
    changes.after.indexes[0].kind = Kind::Normal;
    changes.after.indexes[0].fields = vec![Field {
        name: "id".into(),
        ..Default::default()
    }];
    if kind == "POSTGRESQL" {
        changes.after.indexes[0].method = "hash".into();
    }
    let snapshot = save(db, &c, changes).await?;
    if kind == "POSTGRESQL" {
        assert_eq!(snapshot.definition.indexes[0].method, "hash");
    }
    let stale = edit(snapshot.clone());
    db.query(
        &c,
        &format!("CREATE INDEX ix_external ON {qualified}(label)"),
    )
    .await?;
    assert!(
        save(db, &c, stale)
            .await
            .unwrap_err()
            .to_string()
            .contains("其他操作修改")
    );
    let snapshot = load(db, &c, &table).await?;
    let mut changes = edit(snapshot);
    changes.after.indexes.retain(|i| i.name != "ix_external");
    let snapshot = save(db, &c, changes).await?;
    assert_eq!(snapshot.definition.indexes.len(), 1);
    // Opaque expressions/predicates must survive an ordinary column comment and rename-only edits.
    let special = if kind == "MYSQL" {
        format!("CREATE INDEX ix_special ON {qualified}(label) INVISIBLE")
    } else {
        format!("CREATE INDEX ix_special ON {qualified}(lower(label)) INCLUDE(id) WHERE id>0")
    };
    db.query(&c, &special).await?;
    let snapshot = load(db, &c, &table).await?;
    let special = snapshot
        .definition
        .indexes
        .iter()
        .find(|i| i.name == "ix_special")
        .unwrap();
    assert!(!special.opaque.is_empty());
    let signature = special.signature.clone();
    let mut changes = edit(snapshot);
    changes.after.columns[1].comment = "keep special index".into();
    let snapshot = save(db, &c, changes).await?;
    assert_eq!(
        snapshot
            .definition
            .indexes
            .iter()
            .find(|i| i.name == "ix_special")
            .unwrap()
            .signature,
        signature
    );
    let mut changes = edit(snapshot);
    changes
        .after
        .indexes
        .iter_mut()
        .find(|i| i.name == "ix_special")
        .unwrap()
        .name = "ix_special_new".into();
    let snapshot = save(db, &c, changes).await?;
    assert!(
        snapshot
            .definition
            .indexes
            .iter()
            .any(|i| i.name == "ix_special_new" && !i.opaque.is_empty())
    );
    if kind == "MYSQL" {
        // FULLTEXT and HASH are real server paths, with their own engine requirements.
        let mut changes = edit(snapshot);
        changes.after.indexes.push(Index {
            name: "ix_fulltext".into(),
            kind: Kind::Fulltext,
            fields: vec![Field {
                name: "label".into(),
                ..Default::default()
            }],
            ..Default::default()
        });
        let snapshot = save(db, &c, changes).await?;
        assert!(
            snapshot
                .definition
                .indexes
                .iter()
                .any(|i| i.kind == Kind::Fulltext)
        );
        let memory = TableId {
            schema: scope.into(),
            name: "index_hash".into(),
        };
        let request = Create {
            auxiliary: Vec::new(),
            table: memory.clone(),
            indexes: vec![Index {
                name: "ix_hash".into(),
                method: "HASH".into(),
                fields: vec![Field {
                    name: "id".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            columns: vec![Column {
                name: "id".into(),
                data_type: "INT".into(),
                ..Default::default()
            }],
            options: tomato_core::table_design::Options {
                engine: "MEMORY".into(),
                ..Default::default()
            },
            comment: String::new(),
        };
        db.workbench(&c, DatabaseAction::CreateTable { request })
            .await?;
        let snapshot = load(db, &c, &memory).await?;
        assert_eq!(snapshot.definition.indexes[0].method, "HASH");
        let spatial = TableId {
            schema: scope.into(),
            name: "index_spatial".into(),
        };
        let request = Create {
            auxiliary: Vec::new(),
            table: spatial.clone(),
            indexes: vec![Index {
                name: "ix_point".into(),
                kind: Kind::Spatial,
                fields: vec![Field {
                    name: "location".into(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            columns: vec![Column {
                name: "location".into(),
                data_type: "POINT".into(),
                not_null: true,
                ..Default::default()
            }],
            options: Default::default(),
            comment: String::new(),
        };
        db.workbench(&c, DatabaseAction::CreateTable { request })
            .await?;
        let snapshot = load(db, &c, &spatial).await?;
        assert_eq!(snapshot.definition.indexes[0].kind, Kind::Spatial);
    }
    if kind == "POSTGRESQL" {
        let methods = TableId {
            schema: scope.into(),
            name: "index_methods".into(),
        };
        let request = Create {
            auxiliary: Vec::new(),
            table: methods.clone(),
            indexes: [
                ("ix_gist", "gist", "p"),
                ("ix_spgist", "spgist", "p"),
                ("ix_gin", "gin", "doc"),
                ("ix_brin", "brin", "id"),
            ]
            .into_iter()
            .map(|(name, method, field)| Index {
                name: name.into(),
                method: method.into(),
                fields: vec![Field {
                    name: field.into(),
                    ..Default::default()
                }],
                ..Default::default()
            })
            .collect(),
            columns: [("id", "INTEGER"), ("p", "point"), ("doc", "jsonb")]
                .into_iter()
                .map(|(name, ty)| Column {
                    name: name.into(),
                    data_type: ty.into(),
                    ..Default::default()
                })
                .collect(),
            options: Default::default(),
            comment: String::new(),
        };
        db.workbench(&c, DatabaseAction::CreateTable { request })
            .await?;
        let snapshot = load(db, &c, &methods).await?;
        assert_eq!(snapshot.definition.indexes.len(), 4);
        assert!(
            snapshot
                .definition
                .indexes
                .iter()
                .all(|i| i.opaque.is_empty()),
            "{:?}",
            snapshot.definition.indexes
        );
        db.query(
            &c,
            &format!("ALTER TABLE {qualified} ADD CONSTRAINT uq_owned UNIQUE(id,label)"),
        )
        .await?;
        let snapshot = load(db, &c, &table).await?;
        let mut changes = edit(snapshot);
        let constraint = changes
            .after
            .indexes
            .iter_mut()
            .find(|i| i.name == "uq_owned")
            .unwrap();
        assert!(constraint.constraint);
        constraint.name = "uq_renamed".into();
        let snapshot = save(db, &c, changes).await?;
        assert!(
            snapshot
                .definition
                .indexes
                .iter()
                .any(|i| i.name == "uq_renamed" && i.constraint)
        );
        let mut changes = edit(snapshot);
        changes.after.indexes.retain(|i| i.name != "uq_renamed");
        assert!(save(db, &c, changes).await.is_err());
    }
    assert_eq!(
        db.query(&c, &format!("SELECT COUNT(*) FROM {qualified}"))
            .await?[0]
            .rows[0][0],
        "2"
    );
    Ok(())
}
