use super::*;
use tomato_core::backup::{Create, Operation, Restore, Scope};
use tomato_services::backup::{Archive, Control};
#[tokio::test]
#[ignore = "Requires the owned disposable loopback PostgreSQL fixture"]
async fn postgres_nb3_schema_types_dependencies_exact_rows_atomic_restore_and_cancellation()
-> Result<()> {
    let uri = url::Url::parse(&std::env::var("TOMATO_TEST_POSTGRES_URL")?)?;
    anyhow::ensure!(
        uri.host_str() == Some("127.0.0.1") && uri.path().starts_with("/tomato_fixture"),
        "owned loopback fixture required"
    );
    let mut base = Connection::new("POSTGRESQL");
    base.name = "PG backup fixture".into();
    base.set("host", "127.0.0.1");
    base.set("port", uri.port().context("fixture port")?);
    base.set(
        "username",
        percent_encoding::percent_decode_str(uri.username())
            .decode_utf8()?
            .into_owned(),
    );
    base.set(
        "password",
        percent_encoding::percent_decode_str(uri.password().unwrap_or_default())
            .decode_utf8()?
            .into_owned(),
    );
    base.set("database", uri.path().trim_start_matches('/'));
    let database = format!("tomato_backup_{}", uuid::Uuid::new_v4().simple());
    let mut connection = base.clone();
    connection.set("database", database.clone());
    connection.set("queryRowLimit", 3000);
    let mut db = Databases::default();
    db.query(&base, &format!("CREATE DATABASE {database}"))
        .await?;
    let outcome:Result<()>=async {
        db.query(&connection,r#"SET TimeZone='Asia/Shanghai';CREATE SCHEMA "odd.schema";
CREATE TYPE "odd.schema".mood AS ENUM ('中文','quote''\\');
CREATE DOMAIN "odd.schema".positive AS numeric(40,18) CHECK(VALUE>=0);
CREATE TYPE "odd.schema".pair AS (label text,n integer);
CREATE FUNCTION public.bump(n integer) RETURNS integer LANGUAGE SQL IMMUTABLE AS 'SELECT n+1';
CREATE FUNCTION public.mutating() RETURNS integer LANGUAGE SQL AS 'SELECT 1';
CREATE TABLE "odd.schema"."items.dot"(id bigint GENERATED ALWAYS AS IDENTITY (START WITH 5000 INCREMENT BY 3), label text COLLATE "C", payload bytea, amount numeric(40,18), moment timestamptz, duration interval, document jsonb, labels text[], mood "odd.schema".mood, moods "odd.schema".mood[], positive "odd.schema".positive, pair "odd.schema".pair, computed integer GENERATED ALWAYS AS (length(label)) STORED, CONSTRAINT pk PRIMARY KEY(id), CONSTRAINT nonnegative CHECK(amount>=0));
COMMENT ON TABLE "odd.schema"."items.dot" IS '中文表'; COMMENT ON COLUMN "odd.schema"."items.dot".label IS '备注';
CREATE INDEX partial_expr ON "odd.schema"."items.dot"((lower(label))) WHERE label IS NOT NULL AND id<>5000;
CREATE TABLE public.child(id serial PRIMARY KEY,extra integer DEFAULT public.bump(1),parent bigint REFERENCES "odd.schema"."items.dot"(id) DEFERRABLE INITIALLY DEFERRED);
CREATE TABLE public.empty(); INSERT INTO public.empty DEFAULT VALUES; INSERT INTO public.empty DEFAULT VALUES;
CREATE FUNCTION public.audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN NEW.label=NEW.label; RETURN NEW; END $$;
CREATE TRIGGER preserve_label BEFORE UPDATE ON "odd.schema"."items.dot" FOR EACH ROW EXECUTE FUNCTION public.audit();
CREATE TABLE public.partitioned(id integer NOT NULL,value text) PARTITION BY RANGE(id);
CREATE TABLE public.part_low PARTITION OF public.partitioned FOR VALUES FROM (0) TO (10);
CREATE TABLE public.part_high PARTITION OF public.partitioned FOR VALUES FROM (10) TO (20);
CREATE INDEX partitioned_value ON public.partitioned(value);
INSERT INTO public.partitioned VALUES(1,'one'),(2,'two'),(11,'eleven');
CREATE POLICY original_policy ON "odd.schema"."items.dot" AS PERMISSIVE FOR ALL TO PUBLIC USING (true) WITH CHECK (true);
ALTER TABLE "odd.schema"."items.dot" ENABLE ROW LEVEL SECURITY;
CREATE VIEW public.z_first AS SELECT id,label,public.bump(id::integer) AS bumped FROM "odd.schema"."items.dot";
CREATE VIEW public.a_last AS SELECT id,label FROM public.z_first;"#).await?;
        for first in (1..=2001).step_by(250) {
            let last=(first+249).min(2001);
            db.query(&connection,&format!(r#"INSERT INTO "odd.schema"."items.dot"(label,payload,amount,moment,duration,document,labels,mood) SELECT CASE WHEN i%3=0 THEN NULL ELSE E'中文 O\'Reilly\\\x1e\n' END,decode('00ff1e5c27','hex'),123456789012345678901.123456789012345678,'2024-02-29 12:34:56.123456+08','1 year 2 days 03:04:05.123456','{{"中文":[1,null]}}',ARRAY['a,b',E'quote\'\\',NULL],'中文' FROM generate_series({first},{last}) i"#)).await?;
        }
        db.query(&connection,r#"INSERT INTO public.child(parent) SELECT min(id) FROM "odd.schema"."items.dot";SELECT setval('public.child_id_seq',200,false)"#).await?;
        let query=r#"SELECT row_to_json(t)::text FROM "odd.schema"."items.dot" t ORDER BY id"#;
        db.query(&connection,r#"UPDATE "odd.schema"."items.dot" SET moods=ARRAY['中文']::"odd.schema".mood[],positive=123.123456789012345678,pair=ROW('中文',17)::"odd.schema".pair"#).await?;
        db.query(&connection,r#"UPDATE "odd.schema"."items.dot" SET label=repeat('中文',100000),payload=decode(repeat('ab',262144),'hex') WHERE id=5000"#).await?;
        let expected=db.query(&connection,query).await?[0].rows.clone();
        let structure=r#"SELECT jsonb_build_object('comment',obj_description(c.oid),'rls',c.relrowsecurity,'constraints',(SELECT jsonb_agg(pg_get_constraintdef(x.oid,false) ORDER BY x.conname) FROM pg_constraint x WHERE x.conrelid=c.oid),'indexes',(SELECT jsonb_agg(pg_get_indexdef(i.indexrelid) ORDER BY ci.relname) FROM pg_index i JOIN pg_class ci ON ci.oid=i.indexrelid WHERE i.indrelid=c.oid),'triggers',(SELECT jsonb_agg(pg_get_triggerdef(t.oid,false) ORDER BY t.tgname) FROM pg_trigger t WHERE t.tgrelid=c.oid AND NOT t.tgisinternal),'policies',(SELECT jsonb_agg(jsonb_build_array(polname,polpermissive,polcmd,pg_get_expr(polqual,polrelid),pg_get_expr(polwithcheck,polrelid)) ORDER BY polname) FROM pg_policy WHERE polrelid=c.oid),'columns',(SELECT jsonb_agg(jsonb_build_array(a.attname,col_description(c.oid,a.attnum),a.attcollation::regcollation::text) ORDER BY a.attnum) FROM pg_attribute a WHERE a.attrelid=c.oid AND a.attnum>0 AND NOT a.attisdropped))::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='odd.schema' AND c.relname='items.dot'"#;
        let expected_structure=db.query(&connection,structure).await?[0].rows.clone();
        let search_path=db.query(&connection,"SHOW search_path").await?[0].rows.clone();
        let scoped=db.backup_objects_in(&connection,&database,Some("odd.schema")).await?;
        anyhow::ensure!(scoped.len()==1 && scoped[0].table.schema=="odd.schema","schema-filtered backup catalog");
        let objects=db.backup_objects(&connection,&database).await?;
        anyhow::ensure!(db.query(&connection,"SHOW search_path").await?[0].rows==search_path,"catalog preserves session search path");
        anyhow::ensure!(objects.len()==11,"all tables, dependent views and overloaded routines must be listed: {}",objects.len());
        let root=tempfile::tempdir()?;let scope=Scope{connection:connection.name.clone(),database:database.clone(),path:String::new()};
        let create=Create{scope:scope.clone(),filename:"native.nb3".into(),objects,comment:"PG native".into(),lock_tables:false,single_transaction:true};
        let report=db.backup(&connection,root.path().into(),Operation::Create(create.clone()),None).await?;
        anyhow::ensure!(report.records==2007 && report.objects==11,"record counts");
        let mut archive=Archive::open(&report.path)?;
        let meta:tomato_core::backup::Metadata=archive.json("meta.json",false)?;
        let object=meta.objects.iter().find(|o|o.name=="odd.schema.items.dot").context("quoted table")?;
        let definition:tomato_core::backup::Definition=archive.json(&object.metadata.filename,true)?;
        anyhow::ensure!(definition.data.unwrap().len()==3 && !definition.fields.unwrap().contains(&"computed".into()),"streamed pages omit generated fields");drop(archive);
        let destination=format!("{database}_restore");
        db.query(&base,&format!("CREATE DATABASE {destination}")).await?;
        let mut target=connection.clone();target.set("database",destination.clone());
        let destination_scope=Scope{connection:connection.name.clone(),database:destination,path:String::new()};
        let destination_file=root.path().join(destination_scope.file("native")?);
        std::fs::create_dir_all(destination_file.parent().context("destination directory")?)?;
        std::fs::copy(&report.path,&destination_file)?;
        db.query(&target,"SET TimeZone='Asia/Shanghai'").await?;
        db.backup(&target,root.path().into(),Operation::Restore(Restore{scope:destination_scope,name:"native".into(),namespace:None}),None).await?;
        anyhow::ensure!(db.query(&target,query).await?[0].rows==expected,"restore into empty database recreates prerequisite types");
        anyhow::ensure!(db.query(&target,"SELECT count(*)::text FROM public.partitioned").await?[0].value(0,0)==Some("3"),"partition data restored exactly once");
        anyhow::ensure!(db.query(&target,structure).await?[0].rows==expected_structure,"indexes, constraints, triggers, policies, collation and comments restored");
        anyhow::ensure!(db.query(&target,"SELECT count(*)::text FROM pg_inherits i JOIN pg_class c ON c.oid=i.inhrelid WHERE c.relkind='i'").await?[0].value(0,0)==Some("2"),"partition indexes attached without duplicates");
        let restore=Restore{scope:scope.clone(),name:"native".into(),namespace:None};
        db.query(&connection,r#"DROP FUNCTION public.mutating();CREATE FUNCTION public.mutating() RETURNS bigint LANGUAGE SQL AS 'SELECT 99';UPDATE "odd.schema"."items.dot" SET label='modified'"#).await?;
        let report=db.backup(&connection,root.path().into(),Operation::Restore(restore.clone()),None).await?;
        anyhow::ensure!(report.records==2007 && db.query(&connection,query).await?[0].rows==expected,"exact PG type roundtrip");
        anyhow::ensure!(db.query(&connection,"SELECT count(*)::text FROM public.a_last").await?[0].value(0,0)==Some("2001"),"dependent views");
        anyhow::ensure!(db.query(&connection,"SELECT public.mutating()::text,pg_typeof(public.mutating())::text").await?[0].rows[0]==vec!["1","integer"],"restore replaces changed function return type");
        anyhow::ensure!(db.query(&connection,"SELECT public.bump(9)::text,nextval('public.child_id_seq')::text").await?[0].rows[0]==vec!["10","200"],"function and uncalled sequence state");
        anyhow::ensure!(db.query(&connection,r#"SELECT nextval(pg_get_serial_sequence('"odd.schema"."items.dot"','id'))::text"#).await?[0].value(0,0)==Some("11003"),"identity sequence state");
        db.query(&connection,r#"UPDATE "odd.schema"."items.dot" SET label='preserve on cancellation'"#).await?;
        let preserved=db.query(&connection,query).await?[0].rows.clone();
        let cell=std::sync::Arc::new(std::sync::OnceLock::<Control>::new());let callback=cell.clone();let fired=std::sync::atomic::AtomicBool::new(false);
        let control=Control::new(move||{if let Some(control)=callback.get() && control.read(|p|p.records>=256).unwrap_or(false) && !fired.swap(true,std::sync::atomic::Ordering::SeqCst){control.cancel();}});cell.set(control.clone()).unwrap();
        anyhow::ensure!(db.backup(&connection,root.path().into(),Operation::Restore(restore.clone()),Some(control)).await.is_err(),"active restore cancellation");
        anyhow::ensure!(db.query(&connection,query).await?[0].rows==preserved,"cancel rolls back DDL and data");
        db.query(&connection,"CREATE VIEW public.external_dependency AS SELECT * FROM public.a_last").await?;
        anyhow::ensure!(db.backup(&connection,root.path().into(),Operation::Restore(restore.clone()),None).await.is_err(),"external dependency prevents destructive cascade");
        anyhow::ensure!(db.query(&connection,query).await?[0].rows==preserved,"restore failure rolls back");
        db.query(&connection,"DROP VIEW public.external_dependency;BEGIN").await?;
        anyhow::ensure!(db.backup(&connection,root.path().into(),Operation::Restore(restore),None).await.is_err(),"manual transaction preserved");db.query(&connection,"ROLLBACK").await?;
        let saved=std::fs::read(&report.path)?;let control=Control::default();control.cancel();
        anyhow::ensure!(db.backup(&connection,root.path().into(),Operation::Create(create.clone()),Some(control)).await.is_err() && std::fs::read(&report.path)?==saved,"cancel preserves published backup");
        let mut locker=Databases::default();
        locker.query(&connection,r#"BEGIN;LOCK TABLE "odd.schema"."items.dot" IN ACCESS EXCLUSIVE MODE"#).await?;
        let control=Control::default();let cancel=control.clone();
        let observed_connection=connection.clone();
        let cancellation=tokio::spawn(async move {
            let mut observer=Databases::default();
            let result:Result<()>=async {
                let deadline=std::time::Instant::now()+std::time::Duration::from_secs(2);
                loop {
                    let rows=observer.query(&observed_connection,"SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND application_name LIKE 'tomato-backup-%' AND pid<>pg_backend_pid())::text").await?;
                    if rows[0].value(0,0)==Some("true"){cancel.cancel();break Ok(());}
                    anyhow::ensure!(std::time::Instant::now()<deadline,"backup never reached the blocked SQL fixture");
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }.await;
            cancel.cancel();let closed=observer.disconnect(&observed_connection.id).await;result?;closed
        });
        let started=std::time::Instant::now();
        let cancelled=tokio::time::timeout(std::time::Duration::from_secs(2),db.backup(&connection,root.path().into(),Operation::Create(create.clone()),Some(control))).await;
        let drained:Result<()>=async {
            let deadline=std::time::Instant::now()+std::time::Duration::from_secs(1);
            loop {
                let activity=locker.query(&connection,"SELECT pg_stat_clear_snapshot()::text;SELECT count(*)::text FROM pg_stat_activity WHERE datname=current_database() AND application_name LIKE 'tomato-backup-%'").await?;
                if activity.last().context("activity result")?.value(0,0)==Some("0"){break Ok(());}
                anyhow::ensure!(std::time::Instant::now()<deadline,"cancelled backend retained its blocked query");
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }.await;
        let unlock=locker.query(&connection,"ROLLBACK").await;locker.disconnect(&connection.id).await?;cancellation.await??;unlock?;drained?;
        anyhow::ensure!(cancelled.context("blocked SQL did not cancel promptly")?.is_err() && started.elapsed()<std::time::Duration::from_secs(2),"blocked backup releases its own protocol session");
        anyhow::ensure!(db.query(&connection,"SHOW TimeZone").await?[0].value(0,0)==Some("Asia/Shanghai"),"cancellation preserves caller session settings");
        let mut locked=create;locked.filename="locked.nb3".into();locked.lock_tables=true;
        db.backup(&connection,root.path().into(),Operation::Create(locked),None).await?;
        anyhow::ensure!(std::fs::read_dir(root.path().join(scope.directory()?))?.count()==2,"no leaked staging pages");
        Ok(())
    }.await;
    db.disconnect(&connection.id).await?;
    let cleanup_destination = db
        .query(
            &base,
            &format!("DROP DATABASE IF EXISTS {database}_restore"),
        )
        .await;
    let cleanup = db.query(&base, &format!("DROP DATABASE {database}")).await;
    db.disconnect(&base.id).await?;
    outcome?;
    cleanup_destination?;
    cleanup?;
    Ok(())
}
