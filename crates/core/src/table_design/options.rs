//! MySQL table options shared by CREATE, ALTER and the designer's SQL preview.
use super::*;

impl Options {
    pub fn byte_len(&self) -> usize {
        [
            &self.engine,
            &self.charset,
            &self.collation,
            &self.auto_increment,
            &self.row_format,
            &self.avg_row_length,
        ]
        .iter()
        .map(|s| s.len())
        .sum()
    }

    pub(super) fn mysql_clauses(&self, before: Option<&Self>) -> Result<Vec<String>> {
        let mut clauses = Vec::new();
        for (keyword, value, previous, numeric) in [
            ("ENGINE", &self.engine, before.map(|b| &b.engine), false),
            (
                "DEFAULT CHARSET",
                &self.charset,
                before.map(|b| &b.charset),
                false,
            ),
            (
                "COLLATE",
                &self.collation,
                before.map(|b| &b.collation),
                false,
            ),
            (
                "AUTO_INCREMENT",
                &self.auto_increment,
                before.map(|b| &b.auto_increment),
                true,
            ),
            (
                "ROW_FORMAT",
                &self.row_format,
                before.map(|b| &b.row_format),
                false,
            ),
            (
                "AVG_ROW_LENGTH",
                &self.avg_row_length,
                before.map(|b| &b.avg_row_length),
                true,
            ),
        ] {
            let value = value.trim();
            if previous.is_some_and(|p| p.trim().eq_ignore_ascii_case(value)) {
                continue;
            }
            if value.is_empty() {
                ensure!(
                    previous.is_none_or(|p| p.is_empty()),
                    "{keyword} 不能为空；请填写明确的选项值"
                );
                continue;
            }
            if numeric {
                ensure!(
                    value.bytes().all(|b| b.is_ascii_digit()),
                    "{keyword} 必须是非负整数"
                );
                let number = value
                    .parse::<u64>()
                    .map_err(|_| anyhow::anyhow!("{keyword} 超出整数范围"))?;
                if keyword == "AVG_ROW_LENGTH" {
                    ensure!(number <= u32::MAX as u64, "AVG_ROW_LENGTH 超出整数范围");
                }
            } else {
                token(value)?;
            }
            if keyword == "ROW_FORMAT" {
                ensure!(
                    [
                        "DEFAULT",
                        "DYNAMIC",
                        "COMPACT",
                        "COMPRESSED",
                        "REDUNDANT",
                        "FIXED"
                    ]
                    .iter()
                    .any(|s| value.eq_ignore_ascii_case(s)),
                    "行格式无效"
                );
            }
            clauses.push(format!("{keyword}={value}"));
        }
        Ok(clauses)
    }

    /// Inserting data advances the counter without changing the table's structure.
    pub fn same_schema(&self, other: &Self, check_counter: bool) -> bool {
        [
            &self.engine,
            &self.charset,
            &self.collation,
            &self.row_format,
            &self.avg_row_length,
        ]
        .into_iter()
        .zip([
            &other.engine,
            &other.charset,
            &other.collation,
            &other.row_format,
            &other.avg_row_length,
        ])
        .all(|(a, b)| a.eq_ignore_ascii_case(b))
            && (!check_counter || self.auto_increment == other.auto_increment)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_options_reject_injected_tokens_and_overflow_without_ignoring_cleared_values() {
        for value in ["-1", "1; DROP TABLE t", "18446744073709551616"] {
            let options = Options {
                auto_increment: value.into(),
                ..Default::default()
            };
            assert!(options.mysql_clauses(None).is_err());
        }
        let previous = Options {
            engine: "InnoDB".into(),
            ..Default::default()
        };
        assert!(Options::default().mysql_clauses(Some(&previous)).is_err());
        let options = Options {
            engine: "InnoDB;DROP".into(),
            ..Default::default()
        };
        assert!(options.mysql_clauses(None).is_err());
        let mut current = previous.clone();
        current.auto_increment = "900".into();
        assert!(current.same_schema(&previous, false));
        assert!(!current.same_schema(&previous, true));
    }
}
