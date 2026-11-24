//! Real bounded metadata, executable copied SQL and constraint effects.
use super::designer_tests::{edit, load, save};
use super::*;
use tomato_core::table_design::{Dialect, alter::InfoPage, auxiliary};

pub(super) async fn run(
    db: &mut Databases,
    connection: &Connection,
    kind: &str,
    scope: &str,
) -> Result<()> {
    let mut connection = connection.clone();
    if kind == "MYSQL" {
        connection.set("database", scope);
    }
    let c = &connection;
    let dialect = tomato_services::dialect::resolve(kind)?;
    let q = |name: &str| dialect.quote(name);
    let qualified = |name: &str| -> Result<String> { Ok(format!("{}.{}", q(scope)?, q(name)?)) };
    let names = |prefix: &str| {
        (0..16)
            .map(|i| {
                let special = match i {
                    0 => "中文,",
                    1 => "a\"b",
                    2 => "a`b",
                    _ => "",
                };
                let mut name = format!("{}{prefix}{i:02}_{special}", if i == 3 { " " } else { "" });
                name.push_str(&"x".repeat((if kind == "MYSQL" { 64 } else { 63 }) - name.len()));
                name
            })
            .collect::<Vec<_>>()
    };
    let locals = names("local");
    let parent_fields = names("parent");
    let local_order = locals.iter().rev().cloned().collect::<Vec<_>>();
    let mut remote_order = parent_fields.clone();
    remote_order.rotate_left(5);
    let field_sql = |fields: &[String]| -> Result<String> {
        Ok(fields
            .iter()
            .map(|name| q(name))
            .collect::<Result<Vec<_>>>()?
            .join(","))
    };
    let columns = |fields: &[String]| -> Result<String> {
        Ok(fields
            .iter()
            .map(|name| Ok(format!("{} INTEGER", q(name)?)))
            .collect::<Result<Vec<_>>>()?
            .join(","))
    };
    let table = TableId {
        schema: scope.into(),
        name: "names_child".into(),
    };
    let parent = qualified("names_parent")?;
    let child = qualified(&table.name)?;
    db.query(c, &format!("CREATE TABLE {parent} ({},PRIMARY KEY({})); CREATE TABLE {child} ({},CONSTRAINT {} FOREIGN KEY({}) REFERENCES {parent}({}) ON DELETE CASCADE ON UPDATE CASCADE)",columns(&parent_fields)?,field_sql(&remote_order)?,columns(&locals)?,q("fk_names")?,field_sql(&local_order)?,field_sql(&remote_order)?)).await?;
    let remote_values = (1..=16).map(|n| n.to_string()).collect::<Vec<_>>();
    let mut values = vec![String::new(); 16];
    for (i, field) in local_order.iter().enumerate() {
        let index = locals.iter().position(|name| name == field).unwrap();
        let reference = parent_fields
            .iter()
            .position(|name| name == &remote_order[i])
            .unwrap();
        values[index] = remote_values[reference].clone();
    }
    db.query(
        c,
        &format!(
            "INSERT INTO {parent} VALUES({}); INSERT INTO {child} VALUES({})",
            remote_values.join(","),
            values.join(",")
        ),
    )
    .await?;
    if kind == "MYSQL" {
        db.query(c, "SET SESSION group_concat_max_len=1024").await?;
        let old = db.query(c, &format!("SELECT GROUP_CONCAT(COLUMN_NAME ORDER BY ORDINAL_POSITION SEPARATOR ',') FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA='{scope}' AND TABLE_NAME='names_child' AND REFERENCED_COLUMN_NAME IS NOT NULL")).await?;
        assert!(
            old[0].rows[0][0].len() < local_order.join(",").len(),
            "legacy GROUP_CONCAT must demonstrate truncation"
        );
    }
    let snapshot = load(db, c, &table).await?;
    let row = &snapshot.foreign_keys.rows[0];
    assert_eq!(row[1], auxiliary::fields_label(&local_order));
    assert_eq!(row[4], auxiliary::fields_label(&remote_order));
    assert_eq!(&row[5..], ["CASCADE", "CASCADE"]);
    assert!(row[1].len() > 1024 && row[4].len() > 1024);
    let mut request = snapshot.definition.clone();
    request.table.name = "names_copy".into();
    let copy_table = request.table.clone();
    let copied = qualified(&request.table.name)?;
    let mut copy_row = row.clone();
    copy_row[0] = "fk_names_copy".into();
    request.auxiliary = vec![auxiliary::Edit {
        before_signature: String::new(),
        page: InfoPage::ForeignKeys,
        before: vec![],
        after: vec![copy_row],
    }];
    let sql = request.sql(Dialect::resolve(kind)?)?;
    for field in local_order.iter().chain(&remote_order) {
        assert!(
            sql.contains(&q(field)?),
            "preview lost a complete field identifier"
        );
    }
    db.query(c, &sql).await?;
    db.query(
        c,
        &format!("INSERT INTO {copied} VALUES({})", values.join(",")),
    )
    .await?;
    let mut invalid = values.clone();
    invalid[15] = "999".into();
    assert!(
        db.query(
            c,
            &format!("INSERT INTO {copied} VALUES({})", invalid.join(","))
        )
        .await
        .is_err()
    );
    db.query(
        c,
        &format!("UPDATE {parent} SET {}=111", q(&remote_order[0])?),
    )
    .await?;
    let row = db
        .query(c, &format!("SELECT {} FROM {copied}", q(&local_order[0])?))
        .await?;
    assert_eq!(row[0].rows[0][0], "111");
    let mut snapshot = load(db, c, &copy_table).await?;
    if kind == "POSTGRESQL" {
        let mut stale = edit(snapshot.clone());
        stale.after.auxiliary = vec![auxiliary::Edit {
            before_signature: snapshot.foreign_keys.signature.clone(),
            page: InfoPage::ForeignKeys,
            before: snapshot.foreign_keys.rows.clone(),
            after: vec![],
        }];
        stale.after.comment = "must not apply".into();
        db.query(
            c,
            &format!(
                "ALTER TABLE {copied} ALTER CONSTRAINT {} DEFERRABLE INITIALLY DEFERRED",
                q("fk_names_copy")?
            ),
        )
        .await?;
        assert!(
            save(db, c, stale)
                .await
                .unwrap_err()
                .to_string()
                .contains("外键或触发器")
        );
        snapshot = load(db, c, &copy_table).await?;
        assert_ne!(snapshot.definition.comment, "must not apply");
    } else {
        db.query(c, "SET SESSION group_concat_max_len=16").await?;
    }
    db.query(c, &format!("DELETE FROM {parent}")).await?;
    for table in [&child, &copied] {
        let rows = db
            .query(c, &format!("SELECT COUNT(*) FROM {table}"))
            .await?;
        assert_eq!(rows[0].rows[0][0], "0");
    }
    let mut change = edit(snapshot.clone());
    change.after.auxiliary = vec![auxiliary::Edit {
        before_signature: snapshot.foreign_keys.signature,
        page: InfoPage::ForeignKeys,
        before: snapshot.foreign_keys.rows,
        after: vec![],
    }];
    let saved = save(db, c, change).await?;
    assert!(saved.foreign_keys.rows.is_empty());
    db.query(
        c,
        &format!("INSERT INTO {copied} VALUES({})", invalid.join(",")),
    )
    .await?;
    Ok(())
}
