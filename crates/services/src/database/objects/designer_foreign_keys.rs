//! Preserve ordered field components; SQL string aggregation loses legal identifiers.
use super::{BUDGET, designer::rows};
use anyhow::{Result, ensure};
use sqlx::AnyConnection;
use tomato_core::{
    table_design::{
        alter::{InfoPage, InfoTable},
        auxiliary,
    },
    workbench::TableId,
};

pub(super) async fn read(
    session: &mut AnyConnection,
    kind: &str,
    table: &TableId,
) -> Result<InfoTable> {
    let sql = match kind {
        "MYSQL" => {
            "SELECT k.CONSTRAINT_NAME,k.COLUMN_NAME,k.REFERENCED_TABLE_SCHEMA,k.REFERENCED_TABLE_NAME,k.REFERENCED_COLUMN_NAME,r.DELETE_RULE,r.UPDATE_RULE,CAST(k.ORDINAL_POSITION AS CHAR) FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.REFERENTIAL_CONSTRAINTS r ON r.CONSTRAINT_SCHEMA=k.CONSTRAINT_SCHEMA AND r.CONSTRAINT_NAME=k.CONSTRAINT_NAME AND r.TABLE_NAME=k.TABLE_NAME WHERE k.TABLE_SCHEMA=? AND k.TABLE_NAME=? AND k.REFERENCED_TABLE_NAME IS NOT NULL ORDER BY k.CONSTRAINT_NAME,k.ORDINAL_POSITION"
        }
        "POSTGRESQL" => {
            "SELECT con.conname::text,a.attname::text,rn.nspname::text,rc.relname::text,ra.attname::text,CASE con.confdeltype WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' ELSE 'SET DEFAULT' END,CASE con.confupdtype WHEN 'a' THEN 'NO ACTION' WHEN 'r' THEN 'RESTRICT' WHEN 'c' THEN 'CASCADE' WHEN 'n' THEN 'SET NULL' ELSE 'SET DEFAULT' END,u.position::text FROM pg_constraint con JOIN pg_class c ON c.oid=con.conrelid JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_class rc ON rc.oid=con.confrelid JOIN pg_namespace rn ON rn.oid=rc.relnamespace JOIN LATERAL unnest(con.conkey,con.confkey) WITH ORDINALITY u(local_number,remote_number,position) ON true JOIN pg_attribute a ON a.attrelid=c.oid AND a.attnum=u.local_number JOIN pg_attribute ra ON ra.attrelid=rc.oid AND ra.attnum=u.remote_number WHERE n.nspname=$1 AND c.relname=$2 AND con.contype='f' ORDER BY con.conname,u.position"
        }
        _ => anyhow::bail!("当前数据库不支持此外键目录"),
    };
    project(rows(session, sql, table).await?)
}
struct Group {
    name: String,
    fields: Vec<String>,
    owner: String,
    table: String,
    referenced: Vec<String>,
    delete_rule: String,
    update_rule: String,
}
impl Group {
    fn into_row(self) -> Vec<String> {
        vec![
            self.name,
            auxiliary::fields_label(&self.fields),
            self.owner,
            self.table,
            auxiliary::fields_label(&self.referenced),
            self.delete_rule,
            self.update_rule,
        ]
    }
}
fn append(output: &mut Vec<Vec<String>>, group: Group, retained: &mut usize) -> Result<()> {
    let row = group.into_row();
    *retained += row.capacity() * std::mem::size_of::<String>()
        + row.iter().map(String::capacity).sum::<usize>();
    let capacity = output.capacity();
    output.push(row);
    *retained += (output.capacity() - capacity) * std::mem::size_of::<Vec<String>>();
    ensure!(*retained <= BUDGET, "外键结构元数据超过 8 MiB");
    Ok(())
}
fn project(rows: Vec<Vec<String>>) -> Result<InfoTable> {
    let mut output = Vec::new();
    let mut current: Option<Group> = None;
    let mut retained = 0;
    for row in rows {
        let [
            name,
            field,
            owner,
            table,
            referenced,
            delete_rule,
            update_rule,
            position,
        ]: [String; 8] = row
            .try_into()
            .map_err(|_| anyhow::anyhow!("外键元数据列数不完整"))?;
        let position = position.parse::<usize>()?;
        if let Some(group) = &mut current
            && group.name == name
        {
            ensure!(
                group.owner == owner
                    && group.table == table
                    && group.delete_rule == delete_rule
                    && group.update_rule == update_rule
                    && position == group.fields.len() + 1,
                "外键字段顺序或引用信息不一致，请刷新"
            );
            ensure!(group.fields.len() < 64, "外键字段超过 64 项");
            group.fields.push(field);
            group.referenced.push(referenced);
        } else {
            ensure!(position == 1, "外键字段顺序不完整，请刷新");
            if let Some(group) = current.take() {
                append(&mut output, group, &mut retained)?;
            }
            current = Some(Group {
                name,
                fields: vec![field],
                owner,
                table,
                referenced: vec![referenced],
                delete_rule,
                update_rule,
            });
        }
    }
    if let Some(group) = current {
        append(&mut output, group, &mut retained)?;
    }
    Ok(InfoTable {
        headers: auxiliary::headers(InfoPage::ForeignKeys),
        rows: output,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(position: &str, field: &str, referenced: &str) -> Vec<String> {
        [
            "fk", field, "remote", "parent", referenced, "CASCADE", "SET NULL", position,
        ]
        .map(String::from)
        .to_vec()
    }
    #[test]
    fn ordered_components_keep_lossless_names_and_independent_foreign_keys() -> Result<()> {
        let mut other = row("1", "id", "id");
        other[0] = "other".into();
        let result = project(vec![
            row("1", "a,b", "z\"q"),
            row("2", " x ", "中文"),
            other,
        ])?;
        assert_eq!(
            result.rows[0],
            [
                "fk",
                "\"a,b\",\" x \"",
                "remote",
                "parent",
                "\"z\"\"q\",中文",
                "CASCADE",
                "SET NULL"
            ]
        );
        assert_eq!(result.rows[1][1], "id");
        assert!(project(vec![])?.rows.is_empty());
        Ok(())
    }
    #[test]
    fn partial_or_inconsistent_catalog_rows_cannot_generate_wrong_constraints() {
        assert!(project(vec![row("2", "a", "x")]).is_err());
        assert!(project(vec![row("1", "a", "x"), row("3", "b", "y")]).is_err());
        let mut inconsistent = row("2", "b", "y");
        inconsistent[3] = "other_parent".into();
        assert!(project(vec![row("1", "a", "x"), inconsistent]).is_err());
    }
}
