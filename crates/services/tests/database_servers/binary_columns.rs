//! Real BIT/BINARY/BLOB reads, writes and streaming copies on an owned MySQL.
use super::*;
use tomato_core::table_transfer::Request;
use tomato_services::{backup::Control, database::transfer};

async fn page(
    db: &mut Databases,
    c: &Connection,
    table: &TableId,
    offset: u64,
    limit: u32,
) -> Result<TablePage> {
    let request = request(table, offset, limit);
    let DatabaseOutput::Page(page) = db.workbench(c, DatabaseAction::Page { request }).await?
    else {
        anyhow::bail!("Missing binary table page")
    };
    Ok(page)
}
fn request(table: &TableId, offset: u64, limit: u32) -> PageRequest {
    PageRequest {
        table: table.clone(),
        offset,
        limit,
        sort: Some(Sort {
            column: "id".into(),
            descending: false,
        }),
        query: Default::default(),
    }
}
async fn update(
    db: &mut Databases,
    c: &Connection,
    table: &TableId,
    original: Vec<Cell>,
    values: Vec<Cell>,
) -> Result<TablePage> {
    let action = DatabaseAction::Apply {
        table: table.clone(),
        request: request(table, 0, 100),
        changes: vec![RowChange::Update { original, values }],
    };
    let DatabaseOutput::Applied {
        affected,
        page: Some(page),
        refresh_error: None,
        ..
    } = db.workbench(c, action).await?
    else {
        anyhow::bail!("Binary row save did not refresh successfully")
    };
    anyhow::ensure!(affected == 1, "Binary save changed {affected} rows");
    Ok(page)
}

#[tokio::test]
#[ignore = "requires TOMATO_TEST_MYSQL_URL pointing to an owned loopback fixture"]
async fn mysql_bit_widths_binary_null_paging_writes_and_transfer_are_lossless() -> Result<()> {
    let url = url::Url::parse(&std::env::var("TOMATO_TEST_MYSQL_URL")?)?;
    anyhow::ensure!(
        url.host_str() == Some("127.0.0.1") && url.path() == "/tomato_fixture",
        "Owned loopback database required"
    );
    let mut c = Connection::new("MYSQL");
    c.set("host", "127.0.0.1");
    c.set("port", u64::from(url.port().context("Fixture port")?));
    c.set("database", "tomato_fixture");
    c.set(
        "username",
        percent_encoding::percent_decode_str(url.username())
            .decode_utf8()?
            .as_ref(),
    );
    c.set(
        "password",
        percent_encoding::percent_decode_str(url.password().unwrap_or_default())
            .decode_utf8()?
            .as_ref(),
    );
    let mut db = Databases::default();
    let scopes = [
        format!("tomato_binary_src_{}", uuid::Uuid::new_v4().simple()),
        format!("tomato_binary_dst_{}", uuid::Uuid::new_v4().simple()),
    ];
    for scope in &scopes {
        db.query(&c, &format!("CREATE DATABASE `{scope}`")).await?;
    }
    let table = TableId {
        schema: scopes[0].clone(),
        name: "typed_values".into(),
    };
    let qualified = format!("`{}`.`typed_values`", scopes[0]);
    let checked: Result<()> = async {
        db.query(&c, &format!("CREATE TABLE {qualified}(id INT PRIMARY KEY, flag BIT(1), mask BIT(9), wide BIT(64), fixed BINARY(4), payload_var VARBINARY(16), payload BLOB, label VARCHAR(100)); INSERT INTO {qualified} VALUES(1,0,0,0,X'00010200',X'',X'','zero'),(2,1,1,1,X'0000FF00',X'00FF80',X'0080FF000A27','one'),(3,1,15,255,X'',X'0000',X'FFFFFFFF','fifteen'),(4,1,256,18446744073709551615,X'000000FF',NULL,NULL,'high'),(5,NULL,NULL,NULL,NULL,NULL,NULL,NULL)")).await?;
        let b = |bytes: &[u8]| Cell::Binary(bytes.to_vec());
        let t = |value: &str| Cell::Text(value.into());
        let mut expected = vec![
            vec![t("1"), b(&[0]), b(&[0,0]), b(&[0;8]), b(&[0,1,2,0]), b(&[]), b(&[]), t("zero")],
            vec![t("2"), b(&[1]), b(&[0,1]), b(&[0,0,0,0,0,0,0,1]), b(&[0,0,255,0]), b(&[0,255,128]), b(&[0,128,255,0,10,39]), t("one")],
            vec![t("3"), b(&[1]), b(&[0,15]), b(&[0,0,0,0,0,0,0,255]), b(&[0;4]), b(&[0,0]), b(&[255;4]), t("fifteen")],
            vec![t("4"), b(&[1]), b(&[1,0]), b(&[255;8]), b(&[0,0,0,255]), Cell::Null, Cell::Null, t("high")],
            vec![t("5"), Cell::Null, Cell::Null, Cell::Null, Cell::Null, Cell::Null, Cell::Null, Cell::Null],
        ];
        for offset in [0, 2, 4] {
            let loaded = page(&mut db, &c, &table, offset, 2).await?;
            anyhow::ensure!(loaded.total == 5 && !loaded.truncated, "Wrong binary page size");
            let start = offset as usize;
            anyhow::ensure!(loaded.rows == expected[start..(start+2).min(5)], "Binary page lost bytes or width: {:?}", loaded.rows);
        }
        for (column, name) in ["flag", "mask", "wide", "fixed", "payload_var", "payload"].into_iter().enumerate() {
            let column = column + 1;
            for selected in &expected {
                let mut filtered = request(&table, 0, 100);
                filtered.query.filters.push(ColumnFilter { column: name.into(), condition: tomato_core::result_filter::Condition {
                    operator: tomato_core::result_filter::Operator::Equal,
                    expected: if selected[column] == Cell::Null { None } else { Some(selected[column].display()) },
                }});
                let DatabaseOutput::Page(loaded) = db.workbench(&c, DatabaseAction::Page { request: filtered }).await? else { anyhow::bail!("Missing filtered binary page") };
                let matches = expected.iter().filter(|row| row[column] == selected[column]).cloned().collect::<Vec<_>>();
                anyhow::ensure!(loaded.total as usize == matches.len() && loaded.rows == matches, "Server filter changed binary semantics for {name}: {:?}", selected[column]);
            }
        }
        let original = expected[0].clone();
        expected[0][7] = t("text changed; binary untouched");
        let saved = update(&mut db, &c, &table, original, expected[0].clone()).await?;
        anyhow::ensure!(saved.rows == expected, "Text save rewrote binary values");
        let original = expected[1].clone();
        expected[1][1] = b(&[0]);
        expected[1][2] = b(&[1,0]);
        expected[1][3] = b(&[255,255,255,255,255,255,255,254]);
        expected[1][5] = b(&[0,255,0,128]);
        let saved = update(&mut db, &c, &table, original, expected[1].clone()).await.context("saving BIT and binary fields")?;
        anyhow::ensure!(saved.rows == expected, "BIT/binary edit did not roundtrip: {:?}", saved.rows);

        let plain = TableId { schema: scopes[0].clone(), name: "without_pk".into() };
        db.query(&c, &format!("CREATE TABLE `{}`.`without_pk` LIKE {qualified}; ALTER TABLE `{}`.`without_pk` DROP PRIMARY KEY; INSERT INTO `{}`.`without_pk` SELECT * FROM {qualified}", scopes[0], scopes[0], scopes[0])).await?;
        let original = expected[1].clone();
        let mut modified = original.clone(); modified[7] = t("without primary key");
        let saved = update(&mut db, &c, &plain, original, modified.clone()).await.context("matching binary fields without a primary key")?;
        anyhow::ensure!(saved.rows[1] == modified, "Binary original-row match failed");

        let query = db.query(&c, &format!("SELECT id,flag,mask,wide FROM {qualified} WHERE id=2")).await?.remove(0);
        let editing = db.query_editing(&c, &format!("{}.typed_values", scopes[0]), &query).await?.context("BIT query must be editable")?;
        let original = (0..4).map(|column| query.value(0, column).map(str::to_owned)).collect::<Vec<_>>();
        let changed = tomato_services::database::query_rows::Change { original: original.clone(), modified: [(1,Some("1".into())),(2,Some("15".into())),(3,Some(u64::MAX.to_string()))].into(), delete:false };
        anyhow::ensure!(db.save_query_rows(&c, editing.clone(), vec![changed]).await? == 1, "BIT query write did not match exactly one row");
        expected[1][1]=b(&[1]);expected[1][2]=b(&[0,15]);expected[1][3]=b(&[255;8]);
        anyhow::ensure!(page(&mut db,&c,&table,0,100).await?.rows==expected,"Query writeback changed BIT bytes");
        let invalid = tomato_services::database::query_rows::Change { original, modified: [(1,Some("2".into()))].into(), delete:false };
        anyhow::ensure!(db.save_query_rows(&c,editing,vec![invalid]).await.is_err(),"BIT(1) must reject 2");
        for invalid in [b(&[]), b(&[2,0]), b(&[0;9])] {
            let mut values=expected[0].clone();values[2]=invalid;
            let action=DatabaseAction::Apply{table:table.clone(),request:request(&table,0,100),changes:vec![RowChange::Update{original:expected[0].clone(),values}]};
            anyhow::ensure!(db.workbench(&c,action).await.is_err(),"Invalid BIT bytes must not be silently coerced");
        }
        anyhow::ensure!(page(&mut db,&c,&table,0,100).await?.rows==expected,"Rejected BIT edits changed data");

        let wide_table=TableId{schema:scopes[0].clone(),name:"wide_without_pk".into()};
        db.query(&c,&format!("CREATE TABLE `{}`.`wide_without_pk`(wide BIT(64),label VARCHAR(30)); INSERT INTO `{}`.`wide_without_pk` VALUES(18446744073709551614,'same'),(18446744073709551615,'same')",scopes[0],scopes[0])).await?;
        let wide_request=PageRequest{table:wide_table.clone(),offset:0,limit:100,sort:Some(Sort{column:"wide".into(),descending:false}),query:Default::default()};
        let DatabaseOutput::Page(wide_page)=db.workbench(&c,DatabaseAction::Page{request:wide_request.clone()}).await? else {anyhow::bail!("Missing wide page")};
        for selected in &wide_page.rows {
            let mut filtered = wide_request.clone();
            filtered.query.filters.push(ColumnFilter { column: "wide".into(), condition: tomato_core::result_filter::Condition {
                operator: tomato_core::result_filter::Operator::Equal, expected: Some(selected[0].display()),
            }});
            let DatabaseOutput::Page(loaded) = db.workbench(&c, DatabaseAction::Page { request: filtered }).await? else { anyhow::bail!("Missing wide filtered page") };
            anyhow::ensure!(loaded.total == 1 && loaded.rows == vec![selected.clone()], "Adjacent unsigned BIT(64) filters lost exactness");
        }
        let mut values=wide_page.rows[1].clone();values[1]=t("only maximum changed");
        let DatabaseOutput::Applied{page:Some(wide_after),refresh_error:None,..}=db.workbench(&c,DatabaseAction::Apply{table:wide_table,request:wide_request,changes:vec![RowChange::Update{original:wide_page.rows[1].clone(),values:values.clone()}]}).await? else {anyhow::bail!("Wide save did not refresh")};
        anyhow::ensure!(wide_after.rows==vec![wide_page.rows[0].clone(),values],"Adjacent BIT(64) values matched through floating point");

        let mut source = c.clone(); source.set("database", scopes[0].clone());
        let mut target = c.clone(); target.id = Connection::new("MYSQL").id; target.set("database", scopes[1].clone());
        let copy = Request { source, target, source_schema: scopes[0].clone(), target_schema: scopes[1].clone(), tables: vec![table.name.clone()], structure: true, data: true, drop_existing: false, add: true, drop: true, modify: true };
        let control = Control::default();
        let plan = transfer::preview(&copy, &control).await?;
        let report = transfer::execute(&copy, &plan, &control).await?;
        anyhow::ensure!(report.failed.is_empty() && report.records == 5, "Binary transfer failed: {:?}", report.failed);
        let copied = page(&mut db, &c, &TableId { schema: scopes[1].clone(), name: table.name.clone() }, 0, 100).await?;
        anyhow::ensure!(copied.rows == expected, "Streaming transfer lost binary width or bytes");
        let exported=tomato_services::database::inserts_sql("MYSQL",&copied.definition,&copied.rows)?;
        db.query(&c,&format!("DELETE FROM `{}`.`typed_values`",scopes[1])).await?;
        db.query(&c,&exported).await.context("reloading exported BIT and binary SQL")?;
        anyhow::ensure!(page(&mut db,&c,&copied.definition.table,0,100).await?.rows==expected,"SQL export roundtrip changed BIT data");
        Ok(())
    }.await;
    // Release every schema-scoped session (including an error's rollback) before
    // DROP DATABASE on a different session requests metadata locks.
    let disconnected = db.disconnect(&c.id).await;
    let mut cleanup = Ok(());
    for scope in &scopes {
        if let Err(error) = db.query(&c, &format!("DROP DATABASE `{scope}`")).await {
            cleanup = Err(error);
        }
    }
    db.disconnect(&c.id).await?;
    disconnected?;
    checked?;
    cleanup
}
