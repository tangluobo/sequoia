use super::*;
#[tokio::test]
#[ignore = "Requires a disposable loopback MySQL fixture; creates and removes its own database"]
async fn mysql_nb3_original_restore_native_snapshot_streaming_and_cancellation() -> Result<()> {
    use tomato_core::backup::{Create, Kind, Operation, Restore, Scope, Selection};
    use tomato_services::backup::{Archive, Control};
    let uri = url::Url::parse(
        &std::env::var("TOMATO_TEST_MYSQL_URL").context("own fixture URL required")?,
    )?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1")
            && uri.path().trim_start_matches('/').starts_with("tomato_"),
        "dedicated loopback fixture required"
    );
    let decode = |value: &str| {
        percent_encoding::percent_decode_str(value)
            .decode_utf8()
            .map(|value| value.into_owned())
    };
    let mut admin = Connection::new("MYSQL");
    admin.name = "backup fixture".into();
    admin.set("host", "127.0.0.1");
    admin.set("port", u64::from(uri.port().context("port required")?));
    admin.set("username", decode(uri.username())?);
    admin.set("password", decode(uri.password().unwrap_or_default())?);
    admin.set("database", uri.path().trim_start_matches('/'));
    let database = format!("tomato_backup_{}", uuid::Uuid::new_v4().simple());
    let mut connection = admin.clone();
    connection.set("database", database.clone());
    let dialect = tomato_services::dialect::resolve("MYSQL")?;
    let mut db = Databases::default();
    db.query(
        &admin,
        &format!(
            "CREATE DATABASE {} CHARACTER SET utf8mb4",
            dialect.quote(&database)?
        ),
    )
    .await?;
    let outcome:Result<()>=async {
  let root=tempfile::tempdir()?;let scope=Scope {connection:connection.name.clone(),database:database.clone(),path:String::new()};
  let directory=root.path().join(scope.directory()?);std::fs::create_dir_all(&directory)?;
  std::fs::copy(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/backup/original.nb3"),root.path().join(scope.file("original")?))?;
  let restored=db.backup(&connection,root.path().into(),Operation::Restore(Restore {scope:scope.clone(),name:"original".into(),namespace:None}),None).await?;
  anyhow::ensure!(restored.records==3,"original records");
  let original=db.query(&connection,"SELECT id,value FROM legacy_sample ORDER BY id").await?;
  anyhow::ensure!(original[0].value(0,1)==Some("中文 O'Reilly")&&original[0].value(1,1)==Some("one\u{1e}\ntwo"),"original quote and embedded separator");
  db.query(&connection,"CREATE TABLE typed_records(id INT PRIMARY KEY, label LONGTEXT, payload LONGBLOB, amount DECIMAL(40,18), bits BIT(9), day DATE, stamp DATETIME(6), `generated` INT GENERATED ALWAYS AS (LENGTH(label)) STORED)").await?;
  for first in (1..=2001).step_by(250) {
   let values=(first..=(first+249).min(2001)).map(|id|format!("({id},CONCAT('quote''',CHAR(92),CHAR(0),CHAR(30),CHAR(10),CONVERT(X'F09F9880' USING utf8mb4)),X'00FF1E5C27',123456789012345678901.123456789012345678,b'100000001','2024-02-29','2024-02-29 12:34:56.123456')")).collect::<Vec<_>>().join(",");
   db.query(&connection,&format!("INSERT INTO typed_records(id,label,payload,amount,bits,day,stamp) VALUES {values}")).await?;
  }
  db.query(&connection,"CREATE VIEW backup_view AS SELECT id,label FROM typed_records; CREATE FUNCTION backup_function(n INT) RETURNS INT DETERMINISTIC RETURN n+1; CREATE EVENT backup_event ON SCHEDULE AT CURRENT_TIMESTAMP + INTERVAL 1 DAY DISABLE DO SET @backup_fixture=1").await?;
  let query="SELECT id,HEX(label),HEX(payload),CAST(amount AS CHAR),CAST(bits AS UNSIGNED),day,stamp,`generated` FROM typed_records ORDER BY id";
  connection.set("queryRowLimit",3000);
  let before=db.query(&connection,query).await?;
  let objects=[("legacy_sample",Kind::Table),("typed_records",Kind::Table),("backup_view",Kind::View),("backup_function",Kind::Function),("backup_event",Kind::Event)].into_iter().map(|(name,kind)|Selection {table:TableId {schema:database.clone(),name:name.into()},kind}).collect::<Vec<_>>();
  let catalog=db.backup_objects(&connection,&database).await?;
  anyhow::ensure!(catalog.len()==objects.len()&&objects.iter().all(|expected|catalog.iter().any(|item|item.kind==expected.kind&&item.table==expected.table)),"actual backup object catalog differs");
  let create=Create {scope:scope.clone(),filename:"native.nb3".into(),objects,comment:"full fixture".into(),lock_tables:false,single_transaction:true};
  let report=db.backup(&connection,root.path().into(),Operation::Create(create.clone()),None).await?;anyhow::ensure!(report.objects==5&&report.records==2004,"native object/record counts");
  let mut archive=Archive::open(&report.path)?;let metadata:tomato_core::backup::Metadata=archive.json("meta.json",false)?;
  let table=metadata.objects.iter().find(|object|object.name=="typed_records").unwrap();let definition:tomato_core::backup::Definition=archive.json(&table.metadata.filename,true)?;
  anyhow::ensure!(definition.data.as_ref().unwrap().len()==3&&!definition.fields.as_ref().unwrap().iter().any(|field|field=="generated"),"1000-row pages and generated column omission");drop(archive);
  let restored=db.backup(&connection,root.path().into(),Operation::Restore(Restore {scope:scope.clone(),name:"native".into(),namespace:None}),None).await?;anyhow::ensure!(restored.records==2004,"restored record count");
  anyhow::ensure!(db.query(&connection,query).await?[0].rows==before[0].rows,"all exact values including binary, decimal, BIT, dates, generated data");
  anyhow::ensure!(db.query(&connection,"SELECT backup_function(7)").await?[0].value(0,0)==Some("8"),"routine restored");
  let saved=std::fs::read(&report.path)?;
  let (signal,receive)=std::sync::mpsc::sync_channel(1);let barrier=std::sync::Arc::new(std::sync::Barrier::new(2));let waiting=barrier.clone();let calls=std::sync::atomic::AtomicUsize::new(0);
  let control=Control::new(move||{if calls.fetch_add(1,std::sync::atomic::Ordering::SeqCst)==2 {let _=signal.send(());waiting.wait();}});
  let cancelled=control.clone();let thread=std::thread::spawn(move||{receive.recv_timeout(std::time::Duration::from_secs(10))?;cancelled.cancel();barrier.wait();Ok::<_,anyhow::Error>(())});
  let mut cancel_create=create.clone();cancel_create.objects.retain(|object|object.table.name=="typed_records");
  let result=db.backup(&connection,root.path().into(),Operation::Create(cancel_create),Some(control)).await;
  thread.join().map_err(|_|anyhow::anyhow!("cancel observer panicked"))??;anyhow::ensure!(result.is_err(),"running backup must cancel");anyhow::ensure!(std::fs::read(&report.path)?==saved,"cancel preserves previous backup");
  anyhow::ensure!(std::fs::read_dir(&directory)?.count()==2,"no leaked gzip or tar staging files");
  db.query(&connection,"BEGIN").await?;let result=db.backup(&connection,root.path().into(),Operation::Create(create),None).await;anyhow::ensure!(result.is_err(),"manual transaction must remain separate");db.query(&connection,"ROLLBACK").await?;
  Ok(())
 }.await;
    db.disconnect(&connection.id).await?;
    let cleanup = db
        .query(
            &admin,
            &format!("DROP DATABASE {}", dialect.quote(&database)?),
        )
        .await;
    db.disconnect(&admin.id).await?;
    outcome?;
    cleanup?;
    Ok(())
}
