use super::*;
#[path = "objects/create_tests.rs"]
mod create_tests;
#[path = "objects/designer_auxiliary.rs"]
mod designer_auxiliary;
#[path = "objects/designer_foreign_names.rs"]
mod designer_foreign_names;
#[path = "objects/designer_generation.rs"]
mod designer_generation;
#[path = "objects/designer_indexes.rs"]
mod designer_indexes;
#[path = "objects/designer_lazy.rs"]
mod designer_lazy;
#[path = "objects/designer_tests.rs"]
mod designer_tests;
#[path = "objects/preload_tests.rs"]
mod preload_tests;

async fn catalog(kind: &str, variable: &str) -> Result<()> {
    let uri = url::Url::parse(&std::env::var(variable).context("isolated catalog URL required")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1") && uri.path() == "/tomato_fixture",
        "dedicated loopback fixture required"
    );
    let mut connection = Connection::new(kind);
    connection.set("host", "127.0.0.1");
    connection.set("port", u64::from(uri.port().context("port required")?));
    connection.set(
        "username",
        percent_encoding::percent_decode_str(uri.username())
            .decode_utf8()?
            .as_ref(),
    );
    connection.set(
        "password",
        percent_encoding::percent_decode_str(uri.password().unwrap_or_default())
            .decode_utf8()?
            .as_ref(),
    );
    connection.set("database", "tomato_fixture");
    let scope = format!("tomato_objects_{}", uuid::Uuid::new_v4().simple());
    let dialect = tomato_services::dialect::resolve(kind)?;
    let quote = |name: &str| dialect.quote(name);
    let qualified =
        |name: &str| -> Result<String> { Ok(format!("{}.{}", quote(&scope)?, quote(name)?)) };
    let mut db = Databases::default();
    let scope_kind = if kind == "MYSQL" {
        "DATABASE"
    } else {
        "SCHEMA"
    };
    db.query(
        &connection,
        &format!("CREATE {scope_kind} {}", quote(&scope)?),
    )
    .await?;
    let result:Result<()>=async {
        let parent=qualified("parent,中文")?;
        let child=qualified("child'quoted")?;
        let view=qualified("view中文")?;
        db.query(&connection,&format!("CREATE TABLE {parent} ({} INTEGER NOT NULL{},{} INTEGER NOT NULL,PRIMARY KEY({},{}))",quote("first,key")?,if kind=="MYSQL" {" COMMENT '字段备注'"} else {""},quote("second'key")?,quote("first,key")?,quote("second'key")?)).await?;
        if kind=="POSTGRESQL" {db.query(&connection,&format!("COMMENT ON COLUMN {parent}.{} IS '字段备注'",quote("first,key")?)).await?;}
        db.query(&connection,&format!("CREATE TABLE {child} (a INTEGER,b INTEGER,CONSTRAINT {} FOREIGN KEY(a,b) REFERENCES {parent}({},{}))",quote("fk,中文")?,quote("first,key")?,quote("second'key")?)).await?;
        db.query(&connection,&format!("CREATE VIEW {view} AS SELECT * FROM {parent}")).await?;
        db.query(&connection,&if kind=="MYSQL" {format!("ALTER TABLE {parent} COMMENT='中文备注'")} else {format!("COMMENT ON TABLE {parent} IS '中文备注'")}).await?;
        let DatabaseOutput::Objects(catalog)=db.workbench(&connection,DatabaseAction::Objects{schema:scope.clone()}).await? else {anyhow::bail!("object catalog required")};
        preload_tests::verify(&mut db,&connection,&scope,&catalog).await?;
        assert_eq!(catalog.objects.len(),3);
        assert_eq!(catalog.objects.iter().filter(|item|item.view).count(),1);
        assert!(catalog.objects.iter().all(|item|item.table.schema==scope));
        assert_eq!(catalog.objects.iter().find(|item|item.table.name=="parent,中文").unwrap().comment,"中文备注");
        let fields=&catalog.objects.iter().find(|item|item.table.name=="parent,中文").unwrap().fields;
        assert_eq!(fields.iter().map(|field|field.name.as_str()).collect::<Vec<_>>(),["first,key","second'key"]);
        assert_eq!(fields[0].comment,"字段备注");
        assert!(catalog.objects.iter().find(|item|item.table.name=="view中文").unwrap().data_length.is_empty());
        assert_eq!(catalog.foreign_keys.len(),1);
        let fk=&catalog.foreign_keys[0];
        assert_eq!(fk.name,"fk,中文");assert_eq!(fk.table.name,"child'quoted");assert_eq!(fk.referenced.schema,scope);assert_eq!(fk.referenced.name,"parent,中文");
        assert_eq!(fk.columns,["a","b"]);assert_eq!(fk.referenced_columns,["first,key","second'key"]);
        // The namespace value is bound, never interpolated into either catalog query.
        let DatabaseOutput::Objects(empty)=db.workbench(&connection,DatabaseAction::Objects{schema:if kind=="MYSQL" {scope.clone()} else {"' OR true --".into()}}).await? else {unreachable!()};
        if kind=="POSTGRESQL" {assert!(empty.objects.is_empty());assert!(empty.foreign_keys.is_empty());}
        mutations(&mut db,&connection,kind,&scope,&parent,&child).await?;
        create_tests::run(&mut db,&connection,kind,&scope).await?;
        designer_tests::run(&mut db,&connection,kind,&scope).await?;
        designer_indexes::run(&mut db,&connection,kind,&scope).await?;
        designer_auxiliary::run(&mut db,&connection,kind,&scope).await?;
        designer_foreign_names::run(&mut db,&connection,kind,&scope).await?;
        designer_generation::run(&mut db,&connection,kind,&scope).await?;
        designer_lazy::run(&mut db,&connection,kind,&scope).await?;
        Ok(())
    }.await;
    let cleanup = db
        .query(
            &connection,
            &format!(
                "DROP {scope_kind} {}{}",
                quote(&scope)?,
                if kind == "POSTGRESQL" { " CASCADE" } else { "" }
            ),
        )
        .await;
    let disconnect = db.disconnect(&connection.id).await;
    result?;
    cleanup?;
    disconnect?;
    Ok(())
}
async fn mutations(
    db: &mut Databases,
    connection: &Connection,
    kind: &str,
    scope: &str,
    parent: &str,
    child: &str,
) -> Result<()> {
    use tomato_core::database_objects::{Mutation, Request, Target};
    let target = |name: &str, view: bool| Target {
        table: TableId {
            schema: scope.into(),
            name: name.into(),
        },
        view,
    };
    let action = |targets, mutation| DatabaseAction::ObjectMutation {
        schema: scope.into(),
        request: Request { targets, mutation },
    };
    let mut routed = connection.clone();
    if kind == "MYSQL" {
        routed.set("database", scope);
    }
    db.query(
        connection,
        &format!("INSERT INTO {parent} VALUES(1,1); INSERT INTO {child} VALUES(1,1)"),
    )
    .await?;
    db.workbench(
        &routed,
        DatabaseAction::Transaction {
            action: TransactionAction::Begin,
        },
    )
    .await?;
    let transaction: Result<()> = async {
        if kind == "MYSQL" {
            anyhow::ensure!(
                db.workbench(
                    &routed,
                    action(
                        vec![target("child'quoted", false)],
                        Mutation::Rename {
                            name: "must_not_rename".into()
                        }
                    )
                )
                .await
                .is_err(),
                "DDL must not commit the user's transaction"
            );
        }
        let DatabaseOutput::ObjectsChanged(report) = db
            .workbench(
                &routed,
                action(
                    vec![
                        target("absent_target", false),
                        target("child'quoted", false),
                    ],
                    Mutation::Clear,
                ),
            )
            .await?
        else {
            anyhow::bail!("mutation report required")
        };
        anyhow::ensure!(
            report.succeeded.len() == 1 && report.failures.len() == 1 && report.affected == 1,
            "failed sibling must not prevent transactional DELETE"
        );
        let data = db
            .query(&routed, &format!("SELECT COUNT(*) FROM {child}"))
            .await?;
        anyhow::ensure!(
            data[0].value(0, 0) == Some("0"),
            "clear must use the user's session"
        );
        Ok(())
    }
    .await;
    let rollback = db
        .workbench(
            &routed,
            DatabaseAction::Transaction {
                action: TransactionAction::Rollback,
            },
        )
        .await;
    transaction?;
    rollback?;
    let data = db
        .query(&routed, &format!("SELECT COUNT(*) FROM {child}"))
        .await?;
    assert_eq!(data[0].value(0, 0), Some("1"));
    let DatabaseOutput::ObjectsChanged(report) = db
        .workbench(
            &routed,
            action(vec![target("view中文", true)], Mutation::Clear),
        )
        .await?
    else {
        unreachable!()
    };
    assert!(report.succeeded.is_empty() && report.failures.is_empty());
    let DatabaseOutput::ObjectsChanged(report) = db
        .workbench(
            &routed,
            action(vec![target("child'quoted", false)], Mutation::Truncate),
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(report.succeeded.len(), 1);
    assert!(report.failures.is_empty());
    let data = db
        .query(&routed, &format!("SELECT COUNT(*) FROM {child}"))
        .await?;
    assert_eq!(data[0].value(0, 0), Some("0"));
    let renamed = "renamed`\"中文";
    let DatabaseOutput::ObjectsChanged(report) = db
        .workbench(
            &routed,
            action(
                vec![target("child'quoted", false)],
                Mutation::Rename {
                    name: renamed.into(),
                },
            ),
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(report.succeeded.len(), 1);
    assert!(report.failures.is_empty());
    let DatabaseOutput::Objects(catalog) = db
        .workbench(
            &routed,
            DatabaseAction::Objects {
                schema: scope.into(),
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert!(
        catalog
            .objects
            .iter()
            .any(|object| object.table.name == renamed)
    );
    assert_eq!(catalog.foreign_keys[0].table.name, renamed);
    let wrong = Target {
        table: TableId {
            schema: "another_schema".into(),
            name: "parent,中文".into(),
        },
        view: false,
    };
    assert!(
        db.workbench(&routed, action(vec![wrong], Mutation::Drop))
            .await
            .is_err()
    );
    let DatabaseOutput::ObjectsChanged(report) = db
        .workbench(
            &routed,
            action(
                vec![
                    target("absent_target", false),
                    target("view中文", true),
                    target(renamed, false),
                ],
                Mutation::Drop,
            ),
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(report.succeeded.len(), 2);
    assert_eq!(report.failures.len(), 1);
    let DatabaseOutput::Objects(catalog) = db
        .workbench(
            &routed,
            DatabaseAction::Objects {
                schema: scope.into(),
            },
        )
        .await?
    else {
        unreachable!()
    };
    assert_eq!(catalog.objects.len(), 1);
    assert_eq!(catalog.objects[0].table.name, "parent,中文");
    assert!(catalog.foreign_keys.is_empty());
    Ok(())
}
#[tokio::test]
#[ignore = "requires dedicated loopback MySQL fixture"]
async fn mysql_objects_preserve_comments_composite_foreign_keys_and_actual_database() -> Result<()>
{
    catalog("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}
#[tokio::test]
#[ignore = "requires dedicated loopback PostgreSQL fixture"]
async fn postgres_objects_preserve_comments_composite_foreign_keys_and_namespace() -> Result<()> {
    catalog("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}
