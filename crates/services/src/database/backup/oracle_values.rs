//! Decode a bounded legacy/standard nb3 value group into native bind parameters.
use anyhow::{Context, Result, ensure};
use tomato_core::workbench::Cell;
struct Input<'a> {
    remaining: &'a str,
    standard: bool,
}
impl Input<'_> {
    fn skip(&mut self) {
        self.remaining = self.remaining.trim_start();
    }
    fn take(&mut self, value: &str) -> bool {
        self.skip();
        if let Some(rest) = self.remaining.strip_prefix(value) {
            self.remaining = rest;
            true
        } else {
            false
        }
    }
    fn quoted(&mut self) -> Result<String> {
        ensure!(self.take("'"), "nb3 字符串缺少引号");
        let mut output = String::new();
        loop {
            let at = self.remaining.find('\'').context("nb3 字符串未闭合")?;
            if !self.standard && at > 0 && self.remaining.as_bytes()[at - 1] == b'\\' {
                output.push_str(&self.remaining[..at - 1]);
                output.push('\'');
                self.remaining = &self.remaining[at + 1..];
                continue;
            }
            output.push_str(&self.remaining[..at]);
            self.remaining = &self.remaining[at + 1..];
            if self.standard && self.remaining.starts_with('\'') {
                output.push('\'');
                self.remaining = &self.remaining[1..];
                continue;
            }
            return Ok(output);
        }
    }
    fn scalar(&mut self) -> Result<Cell> {
        self.skip();
        if self.remaining.starts_with('\'') {
            return Ok(Cell::Text(self.quoted()?));
        }
        let at = self
            .remaining
            .find([',', ')', '('])
            .context("nb3 数值组未闭合")?;
        let token = self.remaining[..at].trim();
        self.remaining = &self.remaining[at..];
        if token.eq_ignore_ascii_case("HEXTORAW") {
            ensure!(self.take("("), "nb3 二进制函数无效");
            let hex = self.quoted()?;
            ensure!(self.take(")"), "nb3 二进制函数未闭合");
            return Ok(Cell::Binary(super::super::workbench::unhex(&hex)?));
        }
        if token.eq_ignore_ascii_case("NULL") {
            Ok(Cell::Null)
        } else if token.eq_ignore_ascii_case("true") {
            Ok(Cell::Boolean(true))
        } else if token.eq_ignore_ascii_case("false") {
            Ok(Cell::Boolean(false))
        } else {
            ensure!(
                tomato_core::backup::sql::is_number(token),
                "nb3 包含不能绑定的 SQL 表达式"
            );
            Ok(Cell::Text(token.into()))
        }
    }
}
pub(super) fn parse(row: &str, standard: bool, fields: usize) -> Result<Vec<Cell>> {
    ensure!(
        row.len() <= 32 * 1024 * 1024 && fields <= 4096,
        "nb3 记录或字段超过限制"
    );
    let mut input = Input {
        remaining: row,
        standard,
    };
    ensure!(input.take("("), "nb3 数据不是数值组");
    let mut values = Vec::with_capacity(fields);
    if !input.remaining.trim_start().starts_with(')') {
        loop {
            ensure!(values.len() < fields, "nb3 数据字段数过多");
            values.push(input.scalar()?);
            if !input.take(",") {
                break;
            }
        }
    }
    ensure!(
        input.take(")") && input.remaining.trim().is_empty() && values.len() == fields,
        "nb3 数据字段数不匹配或包含额外 SQL"
    );
    Ok(values)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binds_precision_unicode_binary_and_both_java_and_standard_quotes() -> Result<()> {
        let values = parse(
            "(12345678901234567890.123456789, '中文 O''Reilly\\', HEXTORAW('00FF'), NULL)",
            true,
            4,
        )?;
        assert_eq!(
            values[0],
            Cell::Text("12345678901234567890.123456789".into())
        );
        assert_eq!(values[1], Cell::Text("中文 O'Reilly\\".into()));
        assert_eq!(values[2], Cell::Binary(vec![0, 255]));
        assert_eq!(values[3], Cell::Null);
        assert_eq!(
            parse("('O\\'Reilly', 'C:\\new\\folder')", false, 2)?,
            vec![
                Cell::Text("O'Reilly".into()),
                Cell::Text("C:\\new\\folder".into())
            ]
        );
        for invalid in [
            "(1); DROP TABLE x",
            "(call())",
            "(HEXTORAW('FFF'))",
            "(1,2)",
            "('unterminated)",
        ] {
            assert!(parse(invalid, true, 1).is_err(), "{invalid}");
        }
        Ok(())
    }
}
