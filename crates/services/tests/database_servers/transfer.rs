//! Real streaming transfers use fresh schema scopes on the owned loopback fixture.
use super::*;
use tomato_core::table_transfer::Request;
use tomato_services::{backup::Control, database::transfer};
async fn roundtrip(kind: &str, variable: &str) -> Result<()> {
    let url = url::Url::parse(&std::env::var(variable)?)?;
    anyhow::ensure!(
        url.host_str() == Some("127.0.0.1") && url.path() == "/tomato_fixture",
        "owned loopback required"
    );
    let mut admin = Connection::new(kind);
    admin.set("host", "127.0.0.1");
    admin.set("port", u64::from(url.port().context("port")?));
    admin.set("database", "tomato_fixture");
    admin.set(
        "username",
        percent_encoding::percent_decode_str(url.username())
            .decode_utf8()?
            .as_ref(),
    );
    admin.set(
        "password",
        percent_encoding::percent_decode_str(url.password().unwrap_or_default())
            .decode_utf8()?
            .as_ref(),
    );
    let mut db = Databases::default();
    let dialect = tomato_services::dialect::resolve(kind)?;
    let scopes = [
        format!("tomato_transfer_src_{}", uuid::Uuid::new_v4().simple()),
        format!("tomato_transfer_dst_{}", uuid::Uuid::new_v4().simple()),
    ];
    let scope_kind = if kind == "MYSQL" {
        "DATABASE"
    } else {
        "SCHEMA"
    };
    for name in &scopes {
        db.query(
            &admin,
            &format!("CREATE {scope_kind} {}", dialect.quote(name)?),
        )
        .await?;
    }
    let qualified = |schema: &str, name: &str| -> Result<String> {
        Ok(format!(
            "{}.{}",
            dialect.quote(schema)?,
            dialect.quote(name)?
        ))
    };
    let name = "rows'中文";
    let result:Result<()>=async{
        let source_table=qualified(&scopes[0],name)?;let target_table=qualified(&scopes[1],name)?;
        db.query(&admin,&format!("CREATE TABLE {source_table}(id INTEGER PRIMARY KEY, amount DECIMAL(38,12), payload {}, label VARCHAR(200), note TEXT)",if kind=="MYSQL"{"LONGBLOB"}else{"BYTEA"})).await?;
        for start in (0..600).step_by(100){
            let values=(start..start+100).map(|id|format!("({id},12345678901234567890.123456789012,{},'中文''quote-{id}',{})",if kind=="MYSQL"{"X'00ff8027'"}else{"decode('00ff8027','hex')"},if id%2==0{"NULL"}else{"'NULL'"})).collect::<Vec<_>>().join(",");
            db.query(&admin,&format!("INSERT INTO {source_table} VALUES {values}")).await?;
        }
        let mut source=admin.clone();let mut target=admin.clone();target.id=Connection::new(kind).id;
        if kind=="MYSQL"{source.set("database",scopes[0].clone());target.set("database",scopes[1].clone());}
        let mut request=Request{source,target,source_schema:scopes[0].clone(),target_schema:scopes[1].clone(),tables:vec![name.into()],structure:true,data:true,drop_existing:false,add:true,drop:true,modify:true};
        let control=Control::default();let plan=transfer::preview(&request,&control).await?;
        anyhow::ensure!(plan.preview().contains("CREATE TABLE"));
        let report=transfer::execute(&request,&plan,&control).await?;
        anyhow::ensure!(report.failed.is_empty(),"{:?}",report.failed);anyhow::ensure!(report.records==600);
        let binary=if kind=="MYSQL"{"HEX(payload)"}else{"encode(payload,'hex')"};
        let records=db.query(&admin,&format!("SELECT COUNT(*),MIN(CAST(amount AS {})),MIN({binary}),SUM(CASE WHEN note IS NULL THEN 1 ELSE 0 END) FROM {target_table}",if kind=="MYSQL"{"CHAR"}else{"TEXT"})).await?;
        anyhow::ensure!(records[0].rows[0][0]=="600"&&records[0].rows[0][1]=="12345678901234567890.123456789012"&&records[0].rows[0][2].eq_ignore_ascii_case("00ff8027")&&records[0].rows[0][3]=="300","{:?}",records[0].rows);
        if kind=="POSTGRESQL" {
            let source_identity=qualified(&scopes[0],"identity_generated")?;let target_identity=qualified(&scopes[1],"identity_generated")?;
            db.query(&admin,&format!("CREATE TABLE {source_identity}(id BIGINT GENERATED ALWAYS AS IDENTITY(START WITH 10 INCREMENT BY 5) PRIMARY KEY, qty INTEGER, doubled INTEGER GENERATED ALWAYS AS(qty*2) STORED, label TEXT); INSERT INTO {source_identity}(qty,label) VALUES(2,'first'),(3,'second'); COMMENT ON TABLE {source_identity} IS '表注释'; CREATE INDEX identity_partial ON {source_identity}(qty) WHERE qty>0")).await?;
            let mut identity=request.clone();identity.tables=vec!["identity_generated".into()];
            let plan=transfer::preview(&identity,&Control::default()).await?;let report=transfer::execute(&identity,&plan,&Control::default()).await?;anyhow::ensure!(report.failed.is_empty(),"{:?}",report.failed);
            db.query(&admin,&format!("INSERT INTO {target_identity}(qty,label) VALUES(4,'third')")).await?;
            let result=db.query(&admin,&format!("SELECT id,doubled FROM {target_identity} ORDER BY id")).await?;anyhow::ensure!(result[0].rows==vec![vec!["10","4"],vec!["15","6"],vec!["20","8"]],"{:?}",result[0].rows);
        }
        // Data-only mode preserves additional destination columns and their defaults.
        db.query(&admin,&format!("ALTER TABLE {target_table} ADD COLUMN target_default INTEGER DEFAULT 42")).await?;
        db.query(&admin,&format!("DELETE FROM {target_table}")).await?;
        let mut data_only=request.clone();data_only.structure=false;
        let plan=transfer::preview(&data_only,&Control::default()).await?;
        let report=transfer::execute(&data_only,&plan,&Control::default()).await?;anyhow::ensure!(report.failed.is_empty()&&report.records==600,"{report:?}");
        anyhow::ensure!(db.query(&admin,&format!("SELECT SUM(target_default) FROM {target_table}")).await?[0].rows[0][0]=="25200");
        db.query(&admin,&format!("ALTER TABLE {target_table} DROP COLUMN target_default")).await?;
        if kind=="MYSQL"{
            let myisam=qualified(&scopes[0],"nontransactional")?;
            db.query(&admin,&format!("CREATE TABLE {myisam}(id INTEGER PRIMARY KEY,label VARCHAR(100)) ENGINE=MyISAM; INSERT INTO {myisam} VALUES(1,'kept')")).await?;
            let mut request=request.clone();request.tables=vec!["nontransactional".into()];let plan=transfer::preview(&request,&Control::default()).await?;let report=transfer::execute(&request,&plan,&Control::default()).await?;anyhow::ensure!(report.failed.is_empty()&&report.records==1,"{report:?}");
        }
        // Reviewed plans reject metadata changes before destructive SQL begins.
        let plan=transfer::preview(&request,&Control::default()).await?;
        db.query(&admin,&format!("ALTER TABLE {target_table} ADD COLUMN surprise INTEGER")).await?;
        anyhow::ensure!(transfer::execute(&request,&plan,&Control::default()).await.unwrap_err().to_string().contains("结构已变化"));
        db.query(&admin,&format!("ALTER TABLE {target_table} DROP COLUMN surprise")).await?;
        db.query(&admin,&format!("ALTER TABLE {target_table} ADD COLUMN extra INTEGER")).await?;
        // Existing cross-connection targets synchronize structure and retain data.
        let plan=transfer::preview(&request,&Control::default()).await?;anyhow::ensure!(!plan.tables[0].data&&plan.preview().contains("DROP"));
        let report=transfer::execute(&request,&plan,&Control::default()).await?;anyhow::ensure!(report.failed.is_empty(),"{:?}",report.failed);
        anyhow::ensure!(db.query(&admin,&format!("SELECT COUNT(*) FROM {target_table}")).await?[0].rows[0][0]=="600");
        // Source and target may be separate saved entries pointing to the same table.
        let mut unsafe_request=request.clone();unsafe_request.target=request.source.clone();unsafe_request.target.id=Connection::new(kind).id;unsafe_request.target_schema=unsafe_request.source_schema.clone();unsafe_request.drop_existing=true;
        anyhow::ensure!(transfer::preview(&unsafe_request,&Control::default()).await.is_err());
        // Cancellation after a committed batch retains only complete batches.
        db.query(&admin,&format!("DROP TABLE {target_table}")).await?;request.drop_existing=false;
        let control=Control::default();let observer=control.clone();
        let cancel=tokio::spawn(async move {loop{if observer.read(|p|p.records>=256).unwrap_or(false){observer.cancel();break;}tokio::task::yield_now().await;}});
        let plan=transfer::preview(&request,&control).await?;let report=transfer::execute(&request,&plan,&control).await?;cancel.abort();let _=cancel.await;
        anyhow::ensure!(report.cancelled&&report.records>=256&&report.records<600,"{report:?}");
        let count=db.query(&admin,&format!("SELECT COUNT(*) FROM {target_table}")).await?[0].rows[0][0].parse::<u64>()?;
        anyhow::ensure!(count==report.records,"pending batch must rollback");
        Ok(())
    }.await;
    db.disconnect(&admin.id).await?;
    for name in scopes.iter().rev() {
        db.query(
            &admin,
            &format!(
                "DROP {scope_kind} {}{}",
                dialect.quote(name)?,
                if kind == "MYSQL" { "" } else { " CASCADE" }
            ),
        )
        .await?;
    }
    db.disconnect(&admin.id).await?;
    result
}
#[tokio::test]
#[ignore = "owned loopback MySQL fixture required"]
async fn mysql_streaming_copy_sync_stale_plan_and_batch_cancellation() -> Result<()> {
    roundtrip("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}
#[tokio::test]
#[ignore = "owned loopback PostgreSQL fixture required"]
async fn postgres_streaming_copy_sync_stale_plan_and_batch_cancellation() -> Result<()> {
    roundtrip("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}

#[tokio::test]
#[ignore = "owned MySQL and PostgreSQL loopback fixtures required"]
async fn mysql_postgres_cross_dialect_transfer_preserves_decimal_binary_null_and_unicode()
-> Result<()> {
    let mut connections = Vec::new();
    for (kind, var) in [
        ("MYSQL", "TOMATO_TEST_MYSQL_URL"),
        ("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL"),
    ] {
        let url = url::Url::parse(&std::env::var(var)?)?;
        anyhow::ensure!(url.host_str() == Some("127.0.0.1") && url.path() == "/tomato_fixture");
        let mut c = Connection::new(kind);
        c.set("host", "127.0.0.1");
        c.set("port", u64::from(url.port().context("port")?));
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
        connections.push(c);
    }
    let names = [
        format!("tomato_transfer_cross_a_{}", uuid::Uuid::new_v4().simple()),
        format!("tomato_transfer_cross_b_{}", uuid::Uuid::new_v4().simple()),
        format!("tomato_transfer_cross_c_{}", uuid::Uuid::new_v4().simple()),
    ];
    let mut db = Databases::default();
    db.query(
        &connections[0],
        &format!(
            "CREATE DATABASE `{}`; CREATE DATABASE `{}`",
            names[0], names[2]
        ),
    )
    .await?;
    db.query(&connections[1], &format!("CREATE SCHEMA \"{}\"", names[1]))
        .await?;
    let outcome:Result<()>=async{
        db.query(&connections[0],&format!("CREATE TABLE `{}`.cross_rows(id BIGINT PRIMARY KEY,amount DECIMAL(38,12),payload LONGBLOB,label VARCHAR(100),note TEXT); INSERT INTO `{}`.cross_rows VALUES(1,12345678901234567890.123456789012,X'00ff80','中文''quote',NULL),(2,NULL,X'','NULL','NULL')",names[0],names[0])).await?;
        let mut source=connections[0].clone();source.set("database",names[0].clone());
        let forward=Request{source,target:connections[1].clone(),source_schema:names[0].clone(),target_schema:names[1].clone(),tables:vec!["cross_rows".into()],structure:true,data:true,drop_existing:false,add:true,drop:true,modify:true};
        let plan=transfer::preview(&forward,&Control::default()).await?;let report=transfer::execute(&forward,&plan,&Control::default()).await?;anyhow::ensure!(report.failed.is_empty(),"{:?}",report.failed);
        let mut reverse=forward.clone();reverse.source=connections[1].clone();reverse.target=connections[0].clone();reverse.target.set("database",names[2].clone());reverse.source_schema=names[1].clone();reverse.target_schema=names[2].clone();
        let plan=transfer::preview(&reverse,&Control::default()).await?;let report=transfer::execute(&reverse,&plan,&Control::default()).await?;anyhow::ensure!(report.failed.is_empty(),"{:?}",report.failed);
        let rows=db.query(&connections[0],&format!("SELECT CAST(amount AS CHAR),HEX(payload),label,note FROM `{}`.cross_rows ORDER BY id",names[2])).await?;
        anyhow::ensure!(rows[0].rows[0][0]=="12345678901234567890.123456789012"&&rows[0].rows[0][1]=="00FF80"&&rows[0].rows[0][2]=="中文'quote"&&rows[0].is_null(0,3)&&rows[0].is_null(1,0)&&rows[0].rows[1][1].is_empty()&&rows[0].rows[1][3]=="NULL","{:?}",rows[0].rows);
        Ok(())
    }.await;
    db.disconnect(&connections[0].id).await?;
    db.disconnect(&connections[1].id).await?;
    db.query(
        &connections[0],
        &format!("DROP DATABASE `{}`; DROP DATABASE `{}`", names[2], names[0]),
    )
    .await?;
    db.query(
        &connections[1],
        &format!("DROP SCHEMA \"{}\" CASCADE", names[1]),
    )
    .await?;
    db.disconnect(&connections[0].id).await?;
    db.disconnect(&connections[1].id).await?;
    outcome
}
