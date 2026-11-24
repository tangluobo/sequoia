//! Physical result-column mapping, matching the original QueryResultEditPlan.
use crate::workbench::Column;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Plan {
    pub database_column_names: Vec<String>,
    pub editable_column_indexes: Vec<usize>,
    pub primary_key_columns: Vec<String>,
    pub uses_original_row_match: bool,
    pub status_text: String,
}
fn blank(text: &str) -> bool {
    text.chars().all(|ch|matches!(ch,'\t'..='\r'|'\u{1c}'..=' '|'\u{1680}'|'\u{2000}'..='\u{2006}'|'\u{2008}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{205f}'|'\u{3000}'))
}
fn simple_upper(ch: char) -> char {
    let mut upper = ch.to_uppercase();
    let first = upper.next().unwrap_or(ch);
    if upper.next().is_some() { ch } else { first }
}
fn simple_lower(ch: char) -> char {
    if ch == 'İ' {
        'i'
    } else {
        let mut lower = ch.to_lowercase();
        let first = lower.next().unwrap_or(ch);
        if lower.next().is_some() { ch } else { first }
    }
}
pub fn identifier_eq(left: &str, right: &str) -> bool {
    left.encode_utf16().count() == right.encode_utf16().count()
        && left.chars().zip(right.chars()).all(|(a, b)| {
            a == b
                || simple_upper(a) == simple_upper(b)
                || simple_lower(simple_upper(a)) == simple_lower(simple_upper(b))
        })
}
fn at(values: Option<&[String]>, index: usize) -> &str {
    values
        .and_then(|values| values.get(index))
        .map_or("", String::as_str)
}
fn resolve<'a>(actual: &[&'a str], candidate: &str) -> Option<&'a str> {
    if let Some(name) = actual.iter().copied().find(|name| *name == candidate) {
        return Some(name);
    }
    let mut matched = None;
    for name in actual
        .iter()
        .copied()
        .filter(|name| identifier_eq(name, candidate))
    {
        if matched.is_some_and(|matched| matched != name) {
            return None;
        }
        matched = Some(name);
    }
    matched
}
impl Plan {
    pub fn is_editable(&self) -> bool {
        !self.editable_column_indexes.is_empty()
    }
    pub fn can_delete(&self) -> bool {
        self.is_editable() && !self.primary_key_columns.is_empty()
    }
    pub fn create(
        kind: Option<&str>,
        target: &str,
        labels: &[String],
        source_columns: Option<&[String]>,
        source_tables: Option<&[String]>,
        columns: &[Column],
    ) -> Self {
        let actual: Vec<_> = columns
            .iter()
            .filter(|column| !blank(&column.name))
            .map(|column| column.name.as_str())
            .collect();
        let primary: Vec<_> = columns
            .iter()
            .filter(|column| column.primary_key > 0 && !blank(&column.name))
            .map(|column| column.name.as_str())
            .collect();
        let mut database_column_names: Vec<_> = (0..labels.len())
            .map(|index| format!("\0query-result-column-{index}"))
            .collect();
        let reliable = source_tables.is_some_and(|tables| tables.len() == labels.len());
        let mut mapped = Vec::new();
        let mut indexes = Vec::new();
        for (index, label) in labels.iter().enumerate() {
            let table = at(source_tables, index);
            if reliable && (blank(table) || !identifier_eq(table, target)) {
                continue;
            }
            let column = at(source_columns, index);
            if reliable && blank(column) {
                continue;
            }
            let Some(actual) = resolve(&actual, if blank(column) { label } else { column }) else {
                continue;
            };
            if mapped.contains(&actual) {
                continue;
            }
            database_column_names[index] = actual.into();
            mapped.push(actual);
            indexes.push(index);
        }
        let complete_primary =
            !primary.is_empty() && primary.iter().all(|name| mapped.contains(name));
        let supports_original = matches!(kind, Some("MYSQL" | "POSTGRESQL" | "ORACLE"));
        let original = supports_original
            && reliable
            && indexes.len() == labels.len()
            && actual.iter().all(|name| mapped.contains(name));
        if !complete_primary && !original {
            indexes.clear();
        }
        let status = if !indexes.is_empty() {
            if original && !complete_primary {
                "可编辑 · 无完整主键，保存时按原始整行定位"
            } else {
                "可编辑 · 双击单元格编辑，Ctrl+V 粘贴修改"
            }
        } else if actual.is_empty() {
            "只读结果 · 未找到源表或源表没有可编辑列"
        } else if primary.is_empty() {
            if supports_original {
                "只读结果 · 无主键表需查询全部真实列"
            } else {
                "只读结果 · 表没有主键"
            }
        } else if supports_original {
            "只读结果 · 查询结果需包含完整主键或全部真实列"
        } else {
            "只读结果 · 查询结果需包含完整主键"
        };
        Self {
            database_column_names,
            editable_column_indexes: indexes,
            primary_key_columns: if complete_primary {
                primary.into_iter().map(str::to_owned).collect()
            } else {
                Vec::new()
            },
            uses_original_row_match: original && !complete_primary,
            status_text: status.into(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn query_edit_plan_matches_original_java_physical_mapping_and_safety_rules() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/sql-result/original-edit-plan.json"
        ))
        .unwrap();
        let strings = |value: &serde_json::Value| {
            value.as_array().map(|values| {
                values
                    .iter()
                    .map(|value| value.as_str().unwrap_or_default().to_owned())
                    .collect::<Vec<_>>()
            })
        };
        for case in cases.as_array().unwrap() {
            let labels = strings(&case["labels"]).unwrap_or_default();
            let sources = strings(&case["sources"]);
            let tables = strings(&case["tables"]);
            let columns: Vec<_> = case["columns"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|value| {
                    let name = value["字段名"].as_str()?;
                    Some(Column {
                        name: name.into(),
                        data_type: String::new(),
                        nullable: true,
                        default: None,
                        primary_key: u32::from(value["主键"] == "是"),
                        generated: false,
                        auto_increment: false,
                        binary: false,
                        type_schema: String::new(),
                        type_name: String::new(),
                    })
                })
                .collect();
            let plan = Plan::create(
                case["kind"].as_str(),
                case["target"].as_str().unwrap(),
                &labels,
                sources.as_deref(),
                tables.as_deref(),
                &columns,
            );
            let actual = serde_json::json!({"databaseColumnNames":plan.database_column_names,"editableColumnIndexes":plan.editable_column_indexes,"primaryKeyColumns":plan.primary_key_columns,"usesOriginalRowMatch":plan.uses_original_row_match,"statusText":plan.status_text});
            let mut expected = case["plan"].clone();
            expected["editableColumnIndexes"]
                .as_array_mut()
                .unwrap()
                .sort_by_key(|index| index.as_u64());
            assert_eq!(actual, expected, "{}", case["name"]);
            assert_eq!(plan.is_editable(), case["editable"].as_bool().unwrap());
            assert_eq!(plan.can_delete(), case["delete"].as_bool().unwrap());
        }
    }
}
