//! Copyable designer SQL uses the same statement boundaries as execution.
use super::Dialect;

pub fn render(dialect: Dialect, statements: &[String]) -> String {
    let mut output = String::new();
    for statement in statements {
        let statement = statement.trim_end();
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(statement);
        if matches!(dialect, Dialect::Oracle) && crate::sql_script::is_oracle_procedural(statement)
        {
            // SQL*Plus separators are presentation only. The actor executes
            // each PL/SQL unit directly, retaining its own terminal semicolon.
            output.push_str("\n/");
        } else if !statement.ends_with(';') {
            output.push(';');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{table_design::alter::InfoPage, table_design::auxiliary, workbench::TableId};

    #[test]
    fn oracle_trigger_preview_roundtrips_multiple_blocks_and_trailing_comments()
    -> anyhow::Result<()> {
        let table = TableId {
            schema: "owned".into(),
            name: "items".into(),
        };
        let bodies = [
            "BEGIN :NEW.\"label\" := q'[雪; /]'; END;",
            "DECLARE n NUMBER := 1; BEGIN NULL; END;\n-- keep this comment",
        ];
        let rows = bodies
            .iter()
            .enumerate()
            .map(|(i, body)| {
                vec![
                    format!("tr_{i}"),
                    "BEFORE".into(),
                    "INSERT".into(),
                    (*body).into(),
                ]
            })
            .collect();
        let edit = auxiliary::Edit {
            before_signature: String::new(),
            page: InfoPage::Triggers,
            before: vec![],
            after: rows,
        };
        let plan = edit.plan(Dialect::Oracle, &table)?;
        for (sql, body) in plan.adds.iter().zip(bodies) {
            assert!(
                sql.ends_with(body),
                "PL/SQL terminators and comments must survive execution: {sql}"
            );
        }
        let mut statements = vec!["COMMENT ON TABLE \"owned\".\"items\" IS 'comment'".into()];
        statements.extend(plan.adds);
        statements.push("ALTER TABLE \"owned\".\"items\" ADD \"extra\" NUMBER".into());
        let preview = render(Dialect::Oracle, &statements);
        let parsed: Vec<_> = crate::sql_script::split_for("ORACLE", Some(&preview)).collect();
        assert_eq!(
            parsed,
            statements.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert!(!preview.contains("END;;"));
        Ok(())
    }
}
