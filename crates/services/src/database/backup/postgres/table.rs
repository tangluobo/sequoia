use super::*;
pub(super) async fn definition(
    conn: &mut AnyConnection,
    table: &TableId,
    result: &mut Definition,
) -> Result<()> {
    let name = qualified(table)?;
    let values=rows(conn,"SELECT c.relkind::text,c.relpersistence::text,c.relispartition::text,COALESCE(quote_literal(obj_description(c.oid)),''),EXISTS(SELECT 1 FROM pg_inherits i WHERE i.inhrelid=c.oid)::text,c.relrowsecurity::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2",table).await?;
    let row = values.first().context("表不存在")?;
    let partitioned = workbench::text(row, 0)? == "p";
    let partition = workbench::text(row, 2)? == "true";
    ensure!(
        matches!(workbench::text(row, 0)?.as_str(), "r" | "p")
            && (partition || workbench::text(row, 4)? == "false"),
        "普通继承表需要专门的继承结构备份，拒绝遗漏父表定义"
    );
    let row_security = workbench::text(row, 5)? == "true";
    let unlogged = workbench::text(row, 1)? == "u";
    let comment = workbench::text(row, 3)?;
    let mut columns = Vec::new();
    let mut fields = Vec::new();
    let mut before = Vec::new();
    let mut types_seen = std::collections::HashSet::new();
    let mut after = Vec::new();
    if !comment.is_empty() {
        after.push(format!("COMMENT ON TABLE {name} IS {comment}"));
    }
    let values=rows(conn,"SELECT a.attname::text,format_type(a.atttypid,a.atttypmod),a.attnotnull::text,a.attidentity::text,a.attgenerated::text,COALESCE(pg_get_expr(d.adbin,d.adrelid),''),COALESCE(quote_literal(col_description(c.oid,a.attnum)),''),CASE WHEN a.attcollation<>0 AND a.attcollation<>t.typcollation THEN quote_ident(cn.nspname)||'.'||quote_ident(co.collname) ELSE '' END,tn.nspname::text,t.typtype::text,t.typname::text,t.oid::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_type t ON t.oid=a.atttypid JOIN pg_namespace tn ON tn.oid=t.typnamespace LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum LEFT JOIN pg_collation co ON co.oid=a.attcollation LEFT JOIN pg_namespace cn ON cn.oid=co.collnamespace WHERE n.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum",table).await?;
    for row in values {
        let field = workbench::text(&row, 0)?;
        let quoted = quote(&field)?;
        let data_type = workbench::text(&row, 1)?;
        let identity = workbench::text(&row, 3)?;
        let generated = workbench::text(&row, 4)?;
        let default = workbench::text(&row, 5)?;
        types::append(
            conn,
            workbench::text(&row, 11)?.parse()?,
            &mut before,
            &mut types_seen,
            0,
        )
        .await?;
        let mut column = format!("{quoted} {data_type}");
        let collation = workbench::text(&row, 7)?;
        if !collation.is_empty() {
            column.push_str(&format!(" COLLATE {collation}"));
        }
        if !generated.is_empty() {
            column.push_str(&format!(" GENERATED ALWAYS AS ({default}) STORED"));
        } else {
            fields.push(field.clone());
            let seq = sequence(conn, &name, &field).await?;
            if !identity.is_empty() {
                let seq = seq.context("自增字段没有序列")?;
                column.push_str(&format!(
                    " GENERATED {} AS IDENTITY (SEQUENCE NAME {} {})",
                    if identity == "a" {
                        "ALWAYS"
                    } else {
                        "BY DEFAULT"
                    },
                    seq.name,
                    seq.options
                ));
                after.push(seq.reset);
            } else {
                if let Some(seq) = seq {
                    before.push(format!(
                        "CREATE SEQUENCE {} AS {} {}",
                        seq.name, seq.data_type, seq.options
                    ));
                    after.push(format!(
                        "ALTER SEQUENCE {} OWNED BY {name}.{quoted}",
                        seq.name
                    ));
                    after.push(seq.reset);
                } else if !partition {
                    ensure!(
                        !default.starts_with("nextval("),
                        "字段 {field} 使用共享序列，不能遗漏序列定义；请使用 pg_dump"
                    );
                }
                if !default.is_empty() {
                    column.push_str(&format!(" DEFAULT {default}"));
                }
            }
        }
        if workbench::text(&row, 2)? == "true" {
            column.push_str(" NOT NULL");
        }
        columns.push(column);
        if partition && generated.is_empty() && identity.is_empty() {
            after.push(format!(
                "ALTER TABLE {name} ALTER COLUMN {quoted} {}",
                if default.is_empty() {
                    "DROP DEFAULT".into()
                } else {
                    format!("SET DEFAULT {default}")
                }
            ));
        }
        let comment = workbench::text(&row, 6)?;
        if !comment.is_empty() {
            after.push(format!("COMMENT ON COLUMN {name}.{quoted} IS {comment}"));
        }
    }
    for row in rows(conn,"SELECT quote_ident(con.conname),pg_get_constraintdef(con.oid,false),con.contype::text FROM pg_constraint con JOIN pg_class c ON c.oid=con.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND con.conislocal ORDER BY con.conname",table).await? {
        let constraint=format!("CONSTRAINT {} {}",workbench::text(&row,0)?,workbench::text(&row,1)?);
        if workbench::text(&row,2)?=="f" {result.trigger_ddl.as_mut().unwrap().push(format!("ALTER TABLE {name} ADD {constraint}"));}
        else if partition {result.index_ddl.as_mut().unwrap().push(format!("ALTER TABLE {name} ADD {constraint}"));}
        else {columns.push(constraint);}
    }
    for row in rows(conn,"SELECT pg_get_indexdef(i.indexrelid) FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND NOT EXISTS(SELECT 1 FROM pg_constraint con WHERE con.conindid=i.indexrelid) ORDER BY i.indexrelid",table).await? {after.push(workbench::text(&row,0)?);}
    for row in rows(conn,"SELECT quote_ident(pn.nspname)||'.'||quote_ident(p.relname),quote_ident(cn.nspname)||'.'||quote_ident(ci.relname) FROM pg_index ix JOIN pg_class c ON c.oid=ix.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_class ci ON ci.oid=ix.indexrelid JOIN pg_namespace cn ON cn.oid=ci.relnamespace JOIN pg_inherits i ON i.inhrelid=ix.indexrelid JOIN pg_class p ON p.oid=i.inhparent JOIN pg_namespace pn ON pn.oid=p.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND NOT EXISTS(SELECT 1 FROM pg_constraint con WHERE con.conindid=ix.indexrelid)",table).await? {
        result.trigger_ddl.as_mut().unwrap().push(format!("ALTER INDEX {} ATTACH PARTITION {}",workbench::text(&row,0)?,workbench::text(&row,1)?));
    }
    for row in rows(conn,"SELECT pg_get_triggerdef(t.oid,false),t.tgenabled::text,quote_ident(t.tgname) FROM pg_trigger t JOIN pg_class c ON c.oid=t.tgrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND NOT t.tgisinternal AND t.tgparentid=0 ORDER BY t.tgname",table).await? {
        result.trigger_ddl.as_mut().unwrap().push(workbench::text(&row,0)?);
        let mode=match workbench::text(&row,1)?.as_str(){"D"=>"DISABLE","R"=>"ENABLE REPLICA","A"=>"ENABLE ALWAYS",_=>"ENABLE"};
        result.trigger_ddl.as_mut().unwrap().push(format!("ALTER TABLE {name} {mode} TRIGGER {}",workbench::text(&row,2)?));
    }
    result.ddl = format!(
        "CREATE {}TABLE {name} (\n{}\n)",
        if unlogged { "UNLOGGED " } else { "" },
        columns.join(",\n")
    );
    let details=rows(conn,"SELECT COALESCE(pg_get_partkeydef(c.oid),''),COALESCE(pg_get_expr(c.relpartbound,c.oid),''),COALESCE(array_to_string(c.reloptions,','),''),c.relforcerowsecurity::text,c.relreplident::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2",table).await?;
    let details = details.first().context("表选项不存在")?;
    if partition {
        let parents=rows(conn,"SELECT pn.nspname::text,p.relname::text FROM pg_inherits i JOIN pg_class c ON c.oid=i.inhrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_class p ON p.oid=i.inhparent JOIN pg_namespace pn ON pn.oid=p.relnamespace WHERE n.nspname=$1 AND c.relname=$2",table).await?;
        let parent = parents.first().context("分区父表不存在")?;
        let parent = qualified(&TableId {
            schema: workbench::text(parent, 0)?,
            name: workbench::text(parent, 1)?,
        })?;
        result
            .postgres
            .as_mut()
            .unwrap()
            .dependencies
            .push(format!("relation:{parent}"));
        result.ddl = format!(
            "CREATE TABLE {name} PARTITION OF {parent} {}",
            workbench::text(details, 1)?
        );
    }
    if partitioned {
        result
            .ddl
            .push_str(&format!(" PARTITION BY {}", workbench::text(details, 0)?));
    }
    let options = workbench::text(details, 2)?;
    if !options.is_empty() {
        result.ddl.push_str(&format!(" WITH ({options})"));
    }
    let replica = workbench::text(details, 4)?;
    if replica == "f" || replica == "n" {
        after.push(format!(
            "ALTER TABLE {name} REPLICA IDENTITY {}",
            if replica == "f" { "FULL" } else { "NOTHING" }
        ));
    }
    if replica == "i" {
        for row in rows(conn,"SELECT quote_ident(ci.relname) FROM pg_index i JOIN pg_class c ON c.oid=i.indrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_class ci ON ci.oid=i.indexrelid WHERE n.nspname=$1 AND c.relname=$2 AND i.indisreplident",table).await? {
            result.trigger_ddl.as_mut().unwrap().push(format!("ALTER TABLE {name} REPLICA IDENTITY USING INDEX {}",workbench::text(&row,0)?));
        }
    }
    for row in rows(conn,"SELECT quote_ident(pol.polname),pol.polpermissive::text,pol.polcmd::text,CASE WHEN pol.polroles='{0}'::oid[] THEN 'PUBLIC' ELSE (SELECT string_agg(quote_ident(rolname),',' ORDER BY rolname) FROM pg_roles WHERE oid=ANY(pol.polroles)) END,COALESCE(pg_get_expr(pol.polqual,pol.polrelid),''),COALESCE(pg_get_expr(pol.polwithcheck,pol.polrelid),'') FROM pg_policy pol JOIN pg_class c ON c.oid=pol.polrelid JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 ORDER BY pol.polname",table).await? {
        let command=match workbench::text(&row,2)?.as_str(){"r"=>"SELECT","a"=>"INSERT","w"=>"UPDATE","d"=>"DELETE",_=>"ALL"};
        let mut ddl=format!("CREATE POLICY {} ON {name} AS {} FOR {command} TO {}",workbench::text(&row,0)?,if workbench::text(&row,1)?=="true"{"PERMISSIVE"}else{"RESTRICTIVE"},workbench::text(&row,3)?);
        let using=workbench::text(&row,4)?;if !using.is_empty(){ddl.push_str(&format!(" USING ({using})"));}
        let check=workbench::text(&row,5)?;if !check.is_empty(){ddl.push_str(&format!(" WITH CHECK ({check})"));}
        result.trigger_ddl.as_mut().unwrap().push(ddl);
    }
    if row_security {
        result
            .trigger_ddl
            .as_mut()
            .unwrap()
            .push(format!("ALTER TABLE {name} ENABLE ROW LEVEL SECURITY"));
    }
    if workbench::text(details, 3)? == "true" {
        result
            .trigger_ddl
            .as_mut()
            .unwrap()
            .push(format!("ALTER TABLE {name} FORCE ROW LEVEL SECURITY"));
    }
    result.sub_ddl.as_mut().unwrap().extend(before);
    result.index_ddl.as_mut().unwrap().extend(after);
    result.fields = Some(fields);
    result.postgres.as_mut().unwrap().drop_sql = format!("DROP TABLE IF EXISTS {name}");
    result
        .postgres
        .as_mut()
        .unwrap()
        .dependencies
        .extend(dependencies::table(conn, table).await?);
    Ok(())
}
struct Sequence {
    name: String,
    data_type: String,
    options: String,
    reset: String,
}
async fn sequence(conn: &mut AnyConnection, table: &str, field: &str) -> Result<Option<Sequence>> {
    let rows=workbench::metadata(conn,"SELECT ns.nspname::text,c.relname::text,s.seqtypid::regtype::text,s.seqstart::text,s.seqincrement::text,s.seqmin::text,s.seqmax::text,s.seqcache::text,s.seqcycle::text FROM pg_sequence s JOIN pg_class c ON c.oid=s.seqrelid JOIN pg_namespace ns ON ns.oid=c.relnamespace WHERE s.seqrelid=pg_get_serial_sequence($1,$2)::regclass",&[Cell::Text(table.into()),Cell::Text(field.into())]).await?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    let v = (0..9)
        .map(|i| workbench::text(row, i))
        .collect::<Result<Vec<_>>>()?;
    let name = qualified(&TableId {
        schema: v[0].clone(),
        name: v[1].clone(),
    })?;
    let options = format!(
        "START WITH {} INCREMENT BY {} MINVALUE {} MAXVALUE {} CACHE {} {}",
        v[3],
        v[4],
        v[5],
        v[6],
        v[7],
        if v[8] == "true" { "CYCLE" } else { "NO CYCLE" }
    );
    let state = workbench::metadata(
        conn,
        &format!("SELECT last_value::text,is_called::text FROM {name}"),
        &[],
    )
    .await?;
    let state = state.first().context("序列不存在")?;
    let last = workbench::text(state, 0)?.parse::<i64>()?;
    let called = workbench::text(state, 1)?.parse::<bool>()?;
    Ok(Some(Sequence {
        name: name.clone(),
        data_type: v[2].clone(),
        options,
        reset: format!(
            "SELECT pg_catalog.setval({}::regclass,{last},{called})",
            literal(&name)?
        ),
    }))
}
