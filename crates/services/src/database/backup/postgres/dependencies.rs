//! pg_depend edges determine both creation order and safe reverse drop order.
use super::*;
pub(super) async fn table(conn: &mut AnyConnection, table: &TableId) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let sql = "SELECT DISTINCT pn.nspname::text,p.proname||'('||pg_get_function_identity_arguments(p.oid)||')' FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace CROSS JOIN LATERAL (SELECT a.oid,'pg_attrdef'::regclass AS classid FROM pg_attrdef a WHERE a.adrelid=c.oid UNION ALL SELECT con.oid,'pg_constraint'::regclass FROM pg_constraint con WHERE con.conrelid=c.oid UNION ALL SELECT ix.indexrelid,'pg_class'::regclass FROM pg_index ix WHERE ix.indrelid=c.oid UNION ALL SELECT tr.oid,'pg_trigger'::regclass FROM pg_trigger tr WHERE tr.tgrelid=c.oid) obj JOIN pg_depend d ON d.classid=obj.classid AND d.objid=obj.oid AND d.refclassid='pg_proc'::regclass JOIN pg_proc p ON p.oid=d.refobjid JOIN pg_namespace pn ON pn.oid=p.pronamespace WHERE n.nspname=$1 AND c.relname=$2 AND pn.nspname<>'pg_catalog'";
    for row in rows(conn, sql, table).await? {
        result.push(routine(
            &workbench::text(&row, 0)?,
            &workbench::text(&row, 1)?,
        )?);
    }
    Ok(result)
}
pub(super) fn routine(schema: &str, signature: &str) -> Result<String> {
    Ok(format!(
        "routine:{}",
        qualified(&TableId {
            schema: schema.into(),
            name: signature.into()
        })?
    ))
}
pub(super) async fn function(conn: &mut AnyConnection, selected: &TableId) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let sql = "SELECT DISTINCT rn.nspname::text,r.relname::text FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace JOIN pg_depend d ON d.classid='pg_proc'::regclass AND d.objid=p.oid AND d.refclassid='pg_class'::regclass JOIN pg_class r ON r.oid=d.refobjid JOIN pg_namespace rn ON rn.oid=r.relnamespace WHERE n.nspname=$1 AND p.proname||'('||pg_get_function_identity_arguments(p.oid)||')'=$2 AND r.relkind IN ('r','p','v')";
    for row in rows(conn, sql, selected).await? {
        result.push(format!(
            "relation:{}",
            qualified(&TableId {
                schema: workbench::text(&row, 0)?,
                name: workbench::text(&row, 1)?
            })?
        ));
    }
    let sql = "SELECT DISTINCT rn.nspname::text,r.proname||'('||pg_get_function_identity_arguments(r.oid)||')' FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace JOIN pg_depend d ON d.classid='pg_proc'::regclass AND d.objid=p.oid AND d.refclassid='pg_proc'::regclass JOIN pg_proc r ON r.oid=d.refobjid JOIN pg_namespace rn ON rn.oid=r.pronamespace WHERE n.nspname=$1 AND p.proname||'('||pg_get_function_identity_arguments(p.oid)||')'=$2 AND r.oid<>p.oid AND rn.nspname<>'pg_catalog'";
    for row in rows(conn, sql, selected).await? {
        result.push(routine(
            &workbench::text(&row, 0)?,
            &workbench::text(&row, 1)?,
        )?);
    }
    Ok(result)
}
pub(super) async fn view_functions(
    conn: &mut AnyConnection,
    table: &TableId,
) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for row in rows(conn,"SELECT DISTINCT pn.nspname::text,p.proname||'('||pg_get_function_identity_arguments(p.oid)||')' FROM pg_rewrite r JOIN pg_class v ON v.oid=r.ev_class JOIN pg_namespace n ON n.oid=v.relnamespace JOIN pg_depend d ON d.classid='pg_rewrite'::regclass AND d.objid=r.oid AND d.refclassid='pg_proc'::regclass JOIN pg_proc p ON p.oid=d.refobjid JOIN pg_namespace pn ON pn.oid=p.pronamespace WHERE n.nspname=$1 AND v.relname=$2 AND pn.nspname<>'pg_catalog'",table).await? {result.push(routine(&workbench::text(&row,0)?,&workbench::text(&row,1)?)?);}
    Ok(result)
}
