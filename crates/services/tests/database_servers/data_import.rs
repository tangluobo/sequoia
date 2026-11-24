//! Owned loopback servers only; source parsing runs in the actual desktop worker.
use super::*;
use std::{io::Write, sync::Arc};
use tomato_core::data_import::{Format, Mode, Options, Source};
use tomato_services::{
    data_import::{self as files, Control},
    database::import as loader,
};
async fn roundtrip(kind: &str, key: &str) -> Result<()> {
    let url = url::Url::parse(&std::env::var(key).context("isolated URL required")?)?;
    anyhow::ensure!(
        url.host_str() == Some("127.0.0.1") && url.path() == "/tomato_fixture",
        "owned loopback required"
    );
    anyhow::ensure!(
        std::env::var_os("TOMATO_IMPORT_SOURCE_EXE").is_some(),
        "set the built desktop source worker"
    );
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
    let schema = if kind == "MYSQL" {
        "tomato_fixture"
    } else {
        "public"
    };
    let table = TableId {
        schema: schema.into(),
        name: format!("tomato_import_{}", uuid::Uuid::new_v4().simple()),
    };
    let dialect = tomato_services::dialect::resolve(kind)?;
    let sqltable = format!("{}.{}", dialect.quote(schema)?, dialect.quote(&table.name)?);
    let mut db = Databases::default();
    db.query(&c,&format!("CREATE TABLE {sqltable}(id INTEGER PRIMARY KEY,amount DECIMAL(38,12),payload {},note TEXT,postal VARCHAR(10),extra VARCHAR(20) DEFAULT 'db-default')",if kind=="MYSQL"{"LONGBLOB"}else{"BYTEA"})).await?;
    let result:Result<()>=async {
        let directory=tempfile::tempdir()?;let path=directory.path().join("owned.csv");let mut file=std::fs::File::create_new(&path)?;writeln!(file,"id,amount,payload,note,postal")?;
        for id in 1..=2305 {writeln!(file,"{id},12345678901234567890.123456789012,{},\"第{id}行🙂\",00123",if id%3==0{"\\N"}else{"00ff8027"})?;}drop(file);
        let source=Source{format:Format::Csv,files:vec![path],delimiter:',',..Default::default()};let prepared=Arc::new(tokio::task::spawn_blocking(move||files::prepare(source,&Control::default())).await??);anyhow::ensure!(prepared.tables[0].rows==2305);
        for mode in Mode::ALL {
            db.query(&c,&format!("DELETE FROM {sqltable}; INSERT INTO {sqltable}(id,note)VALUES(-1,'original')")).await?;
            if mode!=Mode::Append {db.query(&c,&format!("INSERT INTO {sqltable}(id,note)VALUES(1,'old')")).await?;}
            let definition=loader::definition(&c,"tomato_fixture",&table,&Control::default()).await?;let target=loader::defaults(0,&prepared.tables[0],table.clone(),Some(definition));let request=loader::Request{connection:c.clone(),database:"tomato_fixture".into(),prepared:prepared.clone(),targets:vec![target],options:Options{mode,..Default::default()}};let handle=tokio::runtime::Handle::current();let report=tokio::task::spawn_blocking(move||loader::run(request,Control::default(),handle)).await??;anyhow::ensure!(report.error.is_none()&&!report.uncertain,"{kind} {mode:?}: {:?}",report.error);
            let expected=match mode {Mode::Append=>2306,Mode::Update=>2,Mode::AppendUpdate|Mode::SkipDuplicates=>2306,Mode::Delete=>1,Mode::Copy=>2305};let count=db.query(&c,&format!("SELECT COUNT(*) FROM {sqltable}")).await?;anyhow::ensure!(count[0].value(0,0).unwrap().parse::<u64>()?==expected,"{kind} {mode:?}: unexpected count");
            if mode!=Mode::Delete {let id=if matches!(mode,Mode::Update){1}else{2};let binary=if kind=="MYSQL"{"HEX(payload)"}else{"encode(payload,'hex')"};let amount=if kind=="MYSQL"{"CAST(amount AS CHAR)"}else{"CAST(amount AS TEXT)"};let row=db.query(&c,&format!("SELECT {amount},{binary},note,postal,extra FROM {sqltable} WHERE id={id}")).await?;anyhow::ensure!(row[0].value(0,0)==Some("12345678901234567890.123456789012"));anyhow::ensure!(row[0].value(0,1).is_some_and(|s|s.eq_ignore_ascii_case("00ff8027")));anyhow::ensure!(row[0].value(0,2)==Some(format!("第{id}行🙂").as_str()));anyhow::ensure!(row[0].value(0,3)==Some("00123"));anyhow::ensure!(row[0].value(0,4)==Some("db-default"));}
        }Ok(())
    }.await;
    db.query(&c, &format!("DROP TABLE {sqltable}")).await?;
    result
}
#[tokio::test]
#[ignore = "requires an owned loopback MySQL fixture and the desktop source worker"]
async fn mysql_data_import_six_modes_exact_decimal_binary_unicode_and_defaults() -> Result<()> {
    roundtrip("MYSQL", "TOMATO_TEST_MYSQL_URL").await
}
#[tokio::test]
#[ignore = "requires an owned loopback PostgreSQL fixture and the desktop source worker"]
async fn postgres_data_import_six_modes_exact_decimal_binary_unicode_and_defaults() -> Result<()> {
    roundtrip("POSTGRESQL", "TOMATO_TEST_POSTGRES_URL").await
}
