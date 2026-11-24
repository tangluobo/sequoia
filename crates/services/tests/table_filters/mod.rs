//! The same >2-page fixture runs in SQLite and the opt-in MySQL/PostgreSQL servers.
use anyhow::{Result, ensure};
use tomato_core::{
    Connection,
    result_filter::{Condition, Operator},
    workbench::*,
};
use tomato_services::database::{Databases, inserts_sql, page_sql};

fn filter(name: &str, operator: Operator, value: Option<&str>) -> ColumnFilter {
    ColumnFilter {
        column: name.into(),
        condition: Condition {
            operator,
            expected: value.map(str::to_owned),
        },
    }
}
async fn load(
    db: &mut Databases,
    connection: &Connection,
    request: &PageRequest,
) -> Result<TablePage> {
    let DatabaseOutput::Page(page) = db
        .workbench(
            connection,
            DatabaseAction::Page {
                request: request.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("missing table page")
    };
    ensure!(
        page.sql
            == page_sql(
                connection.kind(),
                &page.definition,
                &PageRequest {
                    offset: page.offset,
                    ..request.clone()
                }
            )?,
        "Preview is not the executed page plan"
    );
    ensure!(
        !page.sql.split(" FROM ").next().unwrap().contains("CAST"),
        "Projection must retain native columns"
    );
    Ok(page)
}
pub async fn verify(db: &mut Databases, connection: &Connection, schema: &str) -> Result<()> {
    let table = TableId {
        schema: schema.into(),
        name: format!("tomato_filter_{}", uuid::Uuid::new_v4().simple()),
    };
    let dialect = tomato_services::dialect::resolve(connection.kind())?;
    let name = format!("{}.{}", dialect.quote(schema)?, dialect.quote(&table.name)?);
    let binary = if connection.kind() == "POSTGRESQL" {
        "BYTEA"
    } else {
        "BLOB"
    };
    db.query(connection, &format!("CREATE TABLE {name}(id INTEGER PRIMARY KEY, bucket INTEGER, label TEXT, payload {binary}, amount DECIMAL(30,10))")).await?;
    let result: Result<()> = async {
        let mut request = PageRequest {
            table: table.clone(),
            offset: 0,
            limit: 1000,
            sort: None,
            query: Default::default(),
        };
        let initial = load(db, connection, &request).await?;
        let special = "O'Reilly\\100%_中文";
        for chunk in (0..2205).collect::<Vec<_>>().chunks(100) {
            let rows = chunk
                .iter()
                .map(|&id| {
                    vec![
                        Cell::Integer(id),
                        Cell::Integer(id % 7),
                        if id == 2204 {
                            Cell::Null
                        } else {
                            Cell::Text(if id == 2003 {
                                special.into()
                            } else {
                                format!("row {id}")
                            })
                        },
                        if id == 2003 {
                            Cell::Binary(vec![0, 255, 39])
                        } else {
                            Cell::Binary(vec![])
                        },
                        Cell::Text(if id == 2003 {
                            "12345678901234567890.1234567890".into()
                        } else {
                            "1.0".into()
                        }),
                    ]
                })
                .collect::<Vec<_>>();
            db.query(
                connection,
                &inserts_sql(connection.kind(), &initial.definition, &rows)?,
            )
            .await?;
        }
        ensure!(load(db, connection, &request).await?.rows.len() == 1000);
        request.query.filters = vec![filter("id", Operator::Greater, Some("1999"))];
        request.query.sorts = vec![
            Sort {
                column: "bucket".into(),
                descending: true,
            },
            Sort {
                column: "id".into(),
                descending: true,
            },
        ];
        request.limit = 100;
        let page = load(db, connection, &request).await?;
        ensure!(
            page.total == 205 && page.rows.len() == 100,
            "Filter count must span all pages"
        );
        let mut expected: Vec<_> = (2000..2205).collect();
        expected.sort_by_key(|id| (std::cmp::Reverse(id % 7), std::cmp::Reverse(*id)));
        let actual: Vec<_> = page
            .rows
            .iter()
            .map(|row| row[0].display().parse::<i32>())
            .collect::<std::result::Result<_, _>>()?;
        ensure!(actual == expected[..100], "Sort did not apply before LIMIT");
        request.offset = 100;
        let next = load(db, connection, &request).await?;
        let actual: Vec<_> = next
            .rows
            .iter()
            .map(|row| row[0].display().parse::<i32>())
            .collect::<std::result::Result<_, _>>()?;
        ensure!(
            next.total == 205 && actual == expected[100..200],
            "Next page lost its predicates/order"
        );
        let temp = tempfile::tempdir()?;
        let control = tomato_services::data_export::Control::new(|| {});
        let exported = tomato_services::database::export::table(
            connection,
            if connection.kind() == "MYSQL" {
                schema
            } else {
                connection.text("database")
            },
            &table,
            None,
            &request.query,
            temp.path(),
            &control,
        )
        .await?;
        ensure!(
            exported.records == 205,
            "All-record export ignored server-side criteria"
        );
        request.offset = 0;
        request.query = TableQuery {
            filters: vec![filter("label", Operator::Equal, Some(special))],
            ..Default::default()
        };
        let exact = load(db, connection, &request).await?;
        ensure!(
            exact.total == 1 && exact.rows[0][0].display() == "2003",
            "Escaped Unicode equality lost data"
        );
        if connection.kind() == "MYSQL" {
            let mode =
                db.query(connection, "SELECT @@SESSION.sql_mode").await?[0].rows[0][0].clone();
            db.query(connection, "SET SESSION sql_mode='NO_BACKSLASH_ESCAPES'")
                .await?;
            ensure!(
                load(db, connection, &request).await?.total == 1,
                "Filter changed under NO_BACKSLASH_ESCAPES"
            );
            db.query(
                connection,
                &format!("SET SESSION sql_mode='{}'", mode.replace('\'', "''")),
            )
            .await?;
        } else if connection.kind() == "POSTGRESQL" {
            db.query(connection, "SET standard_conforming_strings=off")
                .await?;
            ensure!(
                load(db, connection, &request).await?.total == 1,
                "Filter changed with legacy PostgreSQL strings"
            );
            db.query(connection, "SET standard_conforming_strings=on")
                .await?;
        }
        if connection.kind() != "SQLITE" {
            request.query.filters = vec![filter(
                "amount",
                Operator::Greater,
                Some("12345678901234567890.1234567889"),
            )];
            let decimal = load(db, connection, &request).await?;
            ensure!(
                decimal.total == 1
                    && decimal.rows[0][4].display() == "12345678901234567890.1234567890",
                "Numeric predicate lost precision"
            );
        }
        request.query.filters = vec![filter("label", Operator::Equal, Some("' OR 1=1 --\\"))];
        ensure!(
            load(db, connection, &request).await?.total == 0,
            "Filter values became SQL"
        );
        request.query.filters = vec![filter("label", Operator::Like, Some("100%_"))];
        ensure!(
            load(db, connection, &request).await?.total == 1,
            "Literal LIKE metacharacters were treated as wildcards"
        );
        request.query = TableQuery {
            search: special.into(),
            ..Default::default()
        };
        ensure!(
            load(db, connection, &request).await?.total == 1,
            "Quick search remained local"
        );
        request.query = TableQuery {
            filters: vec![filter("label", Operator::Equal, None)],
            ..Default::default()
        };
        ensure!(
            load(db, connection, &request).await?.total == 1,
            "NULL equality was not IS NULL"
        );
        request.query = TableQuery {
            filters: vec![filter("label", Operator::Equal, Some(""))],
            ..Default::default()
        };
        ensure!(
            load(db, connection, &request).await?.total == 0,
            "Empty string was confused with NULL"
        );
        request.query.filters = vec![filter("payload", Operator::Equal, Some("00ff27"))];
        ensure!(
            load(db, connection, &request).await?.total == 1,
            "Binary filters must use raw byte equality"
        );
        request.query = TableQuery {
            match_any: true,
            filters: vec![
                filter("id", Operator::Equal, Some("1")),
                filter("id", Operator::Equal, Some("2003")),
            ],
            ..Default::default()
        };
        ensure!(
            load(db, connection, &request).await?.total == 2,
            "OR criteria were not grouped"
        );
        request.query.match_any = false;
        ensure!(
            load(db, connection, &request).await?.total == 0,
            "AND criteria not applied"
        );
        request.query = TableQuery {
            filters: vec![filter("id", Operator::Equal, Some("1 OR 1=1"))],
            ..Default::default()
        };
        ensure!(
            db.workbench(
                connection,
                DatabaseAction::Page {
                    request: request.clone()
                }
            )
            .await
            .is_err(),
            "Invalid numeric filter accepted"
        );
        request.query = TableQuery {
            sorts: vec![Sort {
                column: "id; DROP TABLE x".into(),
                descending: false,
            }],
            ..Default::default()
        };
        ensure!(
            db.workbench(
                connection,
                DatabaseAction::Page {
                    request: request.clone()
                }
            )
            .await
            .is_err(),
            "Unknown sort column accepted"
        );
        request.query = Default::default();
        request.offset = 999999;
        ensure!(
            load(db, connection, &request).await?.offset == 2200,
            "Deletion/out-of-range paging should return the last valid page"
        );
        if connection.kind() != "SQLITE" {
            complex_types(db, connection, &table, &name).await?;
        }
        Ok(())
    }
    .await;
    let cleanup = db.query(connection, &format!("DROP TABLE {name}")).await;
    result?;
    cleanup?;
    Ok(())
}

async fn complex_types(
    db: &mut Databases,
    connection: &Connection,
    table: &TableId,
    qualified: &str,
) -> Result<()> {
    let json = r#"{"text":"O'Reilly\\中文","values":[1,null]}"#;
    let mut columns = vec![("doc", "JSON", json)];
    if connection.kind() == "POSTGRESQL" {
        columns.extend([
            ("doc_b", "JSONB", json),
            ("document", "XML", "<root>O'Reilly 中文</root>"),
            ("tags", "TEXT[]", r#"{a,"b,c"}"#),
            ("token", "UUID", "123e4567-e89b-12d3-a456-426614174000"),
            ("special", "DOUBLE PRECISION", "NaN"),
        ]);
    }
    let dialect = tomato_services::dialect::resolve(connection.kind())?;
    let additions = columns
        .iter()
        .map(|(name, ty, _)| Ok(format!("ADD COLUMN {} {ty}", dialect.quote(name)?)))
        .collect::<Result<Vec<_>>>()?
        .join(", ");
    db.query(connection, &format!("ALTER TABLE {qualified} {additions}"))
        .await?;
    let mut request = PageRequest {
        table: table.clone(),
        offset: 0,
        limit: 1000,
        sort: None,
        query: TableQuery {
            filters: vec![filter("id", Operator::Equal, Some("3000"))],
            ..Default::default()
        },
    };
    let definition = load(db, connection, &request).await?.definition;
    let mut row = vec![
        Cell::Integer(3000),
        Cell::Integer(0),
        Cell::Text("complex".into()),
        Cell::Binary(vec![]),
        Cell::Text("1".into()),
    ];
    row.extend(
        columns
            .iter()
            .map(|(_, _, value)| Cell::Text((*value).into())),
    );
    db.query(
        connection,
        &inserts_sql(connection.kind(), &definition, &[row])?,
    )
    .await?;
    let original = load(db, connection, &request).await?;
    for (name, _, _) in &columns {
        let index = original
            .definition
            .columns
            .iter()
            .position(|c| c.name == *name)
            .unwrap();
        let expected = original.rows[0][index].display();
        request.query = TableQuery {
            filters: vec![filter(name, Operator::Equal, Some(&expected))],
            sorts: vec![Sort {
                column: (*name).into(),
                descending: false,
            }],
            ..Default::default()
        };
        let matched = load(db, connection, &request).await?;
        ensure!(
            matched.total == 1 && matched.rows[0][0].display() == "3000",
            "Native complex-field filter failed for {name}"
        );
    }
    if connection.kind() == "POSTGRESQL" {
        for value in ["Infinity", "-Infinity"] {
            db.query(
                connection,
                &format!("UPDATE {qualified} SET special='{value}' WHERE id=3000"),
            )
            .await?;
            request.query = TableQuery {
                filters: vec![filter("special", Operator::Equal, Some(value))],
                ..Default::default()
            };
            ensure!(
                load(db, connection, &request).await?.total == 1,
                "Non-finite PostgreSQL float equality failed"
            );
        }
    }
    Ok(())
}
