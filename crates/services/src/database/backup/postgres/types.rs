//! Object-local prerequisite types, bounded and dependency ordered; no database-wide type cache.
use super::*;
use futures_util::future::BoxFuture;
pub(super) fn append<'a>(
    conn: &'a mut AnyConnection,
    oid: i64,
    output: &'a mut Vec<String>,
    seen: &'a mut std::collections::HashSet<i64>,
    depth: usize,
) -> BoxFuture<'a, Result<()>> {
    Box::pin(async move {
        ensure!(
            depth < 64 && seen.len() < 10_000,
            "PostgreSQL 类型依赖过深或过多"
        );
        if !seen.insert(oid) {
            return Ok(());
        }
        let values=workbench::metadata(conn,"SELECT n.nspname::text,t.typname::text,t.typtype::text,t.typelem::text,t.typbasetype::text,format_type(t.typbasetype,t.typtypmod),t.typnotnull::text,COALESCE(pg_get_expr(t.typdefaultbin,0),''),t.typrelid::text,EXISTS(SELECT 1 FROM pg_depend d WHERE d.classid='pg_type'::regclass AND d.objid=t.oid AND d.deptype='e')::text,COALESCE(quote_ident(cn.nspname)||'.'||quote_ident(co.collname),'') FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace LEFT JOIN pg_collation co ON co.oid=t.typcollation LEFT JOIN pg_namespace cn ON cn.oid=co.collnamespace WHERE t.oid=$1::bigint::oid",&[Cell::Integer(oid)]).await?;
        let row = values.first().context("字段类型不存在")?;
        let v = (0..11)
            .map(|i| workbench::text(row, i))
            .collect::<Result<Vec<_>>>()?;
        if v[0] == "pg_catalog" {
            return Ok(());
        }
        ensure!(
            v[9] == "false",
            "扩展类型 {}.{} 需要目标数据库预装对应扩展，不能遗漏扩展定义",
            v[0],
            v[1]
        );
        let element = v[3].parse::<i64>()?;
        if element != 0 {
            return append(conn, element, output, seen, depth + 1).await;
        }
        let name = qualified(&TableId {
            schema: v[0].clone(),
            name: v[1].clone(),
        })?;
        output.push(format!("CREATE SCHEMA IF NOT EXISTS {}", quote(&v[0])?));
        let (create, check, expected) = match v[2].as_str() {
            "e" => {
                let values=workbench::metadata(conn,"SELECT quote_literal(enumlabel) FROM pg_enum WHERE enumtypid=$1::bigint::oid ORDER BY enumsortorder",&[Cell::Integer(oid)]).await?;
                let labels = values
                    .iter()
                    .map(|r| workbench::text(r, 0))
                    .collect::<Result<Vec<_>>>()?
                    .join(",");
                (
                    format!("CREATE TYPE {name} AS ENUM ({labels})"),
                    format!(
                        "SELECT string_agg(quote_literal(e.enumlabel),',' ORDER BY e.enumsortorder) FROM pg_enum e WHERE e.enumtypid=to_regtype({})",
                        literal(&name)?
                    ),
                    labels,
                )
            }
            "d" => {
                append(conn, v[4].parse()?, output, seen, depth + 1).await?;
                let mut sql = format!("CREATE DOMAIN {name} AS {}", v[5]);
                if !v[10].is_empty() {
                    sql.push_str(&format!(" COLLATE {}", v[10]));
                }
                if !v[7].is_empty() {
                    sql.push_str(&format!(" DEFAULT {}", v[7]));
                }
                if v[6] == "true" {
                    sql.push_str(" NOT NULL");
                }
                let constraints=workbench::metadata(conn,"SELECT quote_ident(conname),pg_get_constraintdef(oid,false) FROM pg_constraint WHERE contypid=$1::bigint::oid ORDER BY conname",&[Cell::Integer(oid)]).await?;
                for row in constraints {
                    sql.push_str(&format!(
                        " CONSTRAINT {} {}",
                        workbench::text(&row, 0)?,
                        workbench::text(&row, 1)?
                    ));
                }
                let fingerprint = "SELECT jsonb_build_object('base',format_type(t.typbasetype,t.typtypmod),'null',t.typnotnull,'default',COALESCE(pg_get_expr(t.typdefaultbin,0),''),'collation',t.typcollation::regcollation::text,'constraints',(SELECT jsonb_agg(jsonb_build_array(conname,pg_get_constraintdef(oid,false)) ORDER BY conname) FROM pg_constraint WHERE contypid=t.oid))::text FROM pg_type t WHERE t.oid=";
                let expected = workbench::metadata(
                    conn,
                    &format!("{fingerprint}$1::bigint::oid"),
                    &[Cell::Integer(oid)],
                )
                .await?;
                (
                    sql,
                    format!("{fingerprint}to_regtype({})", literal(&name)?),
                    workbench::text(expected.first().context("域定义")?, 0)?,
                )
            }
            "c" => {
                let relation = v[8].parse::<i64>()?;
                let rows=workbench::metadata(conn,"SELECT a.attname::text,format_type(a.atttypid,a.atttypmod),a.atttypid::text,c.relkind::text FROM pg_attribute a JOIN pg_class c ON c.oid=a.attrelid WHERE a.attrelid=$1::bigint::oid AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum",&[Cell::Integer(relation)]).await?;
                let mut fields = Vec::new();
                for row in rows {
                    ensure!(
                        workbench::text(&row, 3)? == "c",
                        "引用表行类型需要依赖表先存在"
                    );
                    append(
                        conn,
                        workbench::text(&row, 2)?.parse()?,
                        output,
                        seen,
                        depth + 1,
                    )
                    .await?;
                    fields.push(format!(
                        "{} {}",
                        quote(&workbench::text(&row, 0)?)?,
                        workbench::text(&row, 1)?
                    ));
                }
                let fingerprint = "SELECT jsonb_agg(jsonb_build_array(a.attname,format_type(a.atttypid,a.atttypmod)) ORDER BY a.attnum)::text FROM pg_attribute a JOIN pg_type t ON t.typrelid=a.attrelid WHERE a.attnum>0 AND NOT a.attisdropped AND t.oid=";
                let expected = workbench::metadata(
                    conn,
                    &format!("{fingerprint}$1::bigint::oid"),
                    &[Cell::Integer(oid)],
                )
                .await?;
                (
                    format!("CREATE TYPE {name} AS ({})", fields.join(",")),
                    format!("{fingerprint}to_regtype({})", literal(&name)?),
                    workbench::text(expected.first().context("复合类型定义")?, 0)?,
                )
            }
            _ => anyhow::bail!("自定义类型 {name} 尚不能完整导出，拒绝生成缺失定义的备份"),
        };
        output.push(format!("DO $tomato_type$ BEGIN IF to_regtype({}) IS NULL THEN EXECUTE {}; ELSIF ({check}) IS DISTINCT FROM {} THEN RAISE EXCEPTION '备份类型与现有定义不同'; END IF; END $tomato_type$",literal(&name)?,literal(&create)?,literal(&expected)?));
        ensure!(
            output.iter().map(String::len).sum::<usize>() <= 8 * 1024 * 1024,
            "PostgreSQL 类型定义超过8 MiB"
        );
        Ok(())
    })
}
