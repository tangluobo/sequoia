use serde::{
    Serialize, Serializer,
    ser::{SerializeSeq, SerializeStruct},
};

#[derive(Clone, Debug, Default)]
pub struct ColumnOrigin {
    pub column: String,
    pub table: String,
    pub schema: String,
    pub catalog: String,
    #[doc(hidden)]
    pub pg_reference: Option<(u32, i16)>,
}
#[derive(Clone, Debug, Default)]
pub struct QueryResult {
    pub columns: Vec<String>,
    /// SQL NULL has an empty payload and a bit in null_cells. Empty text has no bit.
    pub rows: Vec<Vec<String>>,
    pub affected: u64,
    pub truncated: bool,
    pub total_rows: u64,
    pub has_result_set: bool,
    pub column_types: Vec<i32>,
    pub origins: Vec<ColumnOrigin>,
    pub origin_error: Option<String>,
    #[doc(hidden)]
    pub null_cells: Vec<u64>,
}
impl QueryResult {
    pub fn row_count(&self) -> u64 {
        self.total_rows.max(self.rows.len() as u64)
    }
    pub fn is_rowset(&self) -> bool {
        self.has_result_set || !self.columns.is_empty()
    }
    pub fn retained_bytes(&self) -> usize {
        let metadata = self.columns.iter().fold(
            self.columns.capacity() * std::mem::size_of::<String>(),
            |bytes, name| bytes.saturating_add(name.capacity()),
        );
        let metadata = self.origins.iter().fold(
            metadata
                .saturating_add(self.origins.capacity() * std::mem::size_of::<ColumnOrigin>())
                .saturating_add(self.column_types.capacity() * std::mem::size_of::<i32>())
                .saturating_add(self.null_cells.capacity() * 8),
            |bytes, origin| {
                bytes
                    .saturating_add(origin.column.capacity())
                    .saturating_add(origin.table.capacity())
                    .saturating_add(origin.schema.capacity())
                    .saturating_add(origin.catalog.capacity())
            },
        );
        self.rows.iter().fold(
            metadata.saturating_add(self.rows.capacity() * std::mem::size_of::<Vec<String>>()),
            |bytes, row| {
                row.iter().fold(
                    bytes.saturating_add(row.capacity() * std::mem::size_of::<String>()),
                    |bytes, value| bytes.saturating_add(value.capacity()),
                )
            },
        )
    }
    pub fn is_null(&self, row: usize, column: usize) -> bool {
        if column >= self.columns.len() {
            return false;
        }
        let Some(index) = row
            .checked_mul(self.columns.len())
            .and_then(|n| n.checked_add(column))
        else {
            return false;
        };
        self.null_cells
            .get(index / 64)
            .is_some_and(|bits| bits & (1 << (index % 64)) != 0)
    }
    pub fn value(&self, row: usize, column: usize) -> Option<&str> {
        if self.is_null(row, column) {
            None
        } else {
            self.rows.get(row)?.get(column).map(String::as_str)
        }
    }
    fn null_words(&self, row: &[Option<String>]) -> usize {
        row.iter().rposition(Option::is_none).map_or(0, |column| {
            (self.rows.len() * self.columns.len() + column) / 64 + 1
        })
    }
    pub(super) fn row_bytes(&self, row: &[Option<String>]) -> usize {
        row.iter()
            .fold(std::mem::size_of::<Vec<String>>(), |bytes, value| {
                bytes
                    .saturating_add(std::mem::size_of::<String>())
                    .saturating_add(value.as_ref().map_or(0, String::capacity))
            })
            .saturating_add(
                self.null_words(row)
                    .saturating_sub(self.null_cells.capacity())
                    * std::mem::size_of::<u64>(),
            )
    }
    pub(super) fn push_row(&mut self, row: Vec<Option<String>>) {
        let words = self.null_words(&row);
        if words > self.null_cells.len() {
            self.null_cells.reserve_exact(words - self.null_cells.len());
            self.null_cells.resize(words, 0);
        }
        let offset = self.rows.len() * self.columns.len();
        self.rows.push(
            row.into_iter()
                .enumerate()
                .map(|(column, value)| {
                    if let Some(value) = value {
                        value
                    } else {
                        let index = offset + column;
                        self.null_cells[index / 64] |= 1 << (index % 64);
                        String::new()
                    }
                })
                .collect(),
        );
    }
}
struct Rows<'a>(&'a QueryResult);
struct Row<'a>(&'a QueryResult, usize);
impl Serialize for Rows<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.rows.len()))?;
        for index in 0..self.0.rows.len() {
            sequence.serialize_element(&Row(self.0, index))?;
        }
        sequence.end()
    }
}
impl Serialize for Row<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.rows[self.1].len()))?;
        for column in 0..self.0.rows[self.1].len() {
            sequence.serialize_element(&self.0.value(self.1, column))?;
        }
        sequence.end()
    }
}
impl Serialize for QueryResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut result = serializer.serialize_struct("QueryResult", 4)?;
        result.serialize_field("columns", &self.columns)?;
        result.serialize_field("rows", &Rows(self))?;
        result.serialize_field("affected", &self.affected)?;
        result.serialize_field("truncated", &self.truncated)?;
        result.end()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn real_query_json_and_csv_keep_null_empty_and_literal_separate() -> anyhow::Result<()> {
        let mut databases = crate::database::Databases::default();
        let connection = tomato_core::Connection::new("SQLITE");
        let results = databases
            .query(
                &connection,
                "SELECT NULL AS missing, '' AS empty, 'NULL' AS literal; SELECT NULL AS second;",
            )
            .await?;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].value(0, 0), None);
        assert_eq!(results[0].value(0, 1), Some(""));
        assert_eq!(results[0].value(0, 2), Some("NULL"));
        assert!(results[1].is_null(0, 0));
        let json = serde_json::to_value(&results)?;
        assert_eq!(json[0]["rows"], serde_json::json!([[null, "", "NULL"]]));
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("query.csv");
        crate::files::export_csv(&path, &results[0].columns, &results[0].rows)?;
        let csv = std::fs::read_to_string(path)?;
        assert_eq!(csv.lines().nth(1), Some("\"\",\"\",\"NULL\""));
        databases.disconnect(&connection.id).await?;
        Ok(())
    }
    #[test]
    fn nullable_results_preserve_empty_and_literal_null_without_null_text_allocations() {
        let mut result = QueryResult {
            columns: vec!["missing".into(), "empty".into(), "literal".into()],
            ..Default::default()
        };
        result.push_row(vec![None, Some(String::new()), Some("NULL".into())]);
        assert_eq!(result.value(0, 0), None);
        assert_eq!(result.value(0, 1), Some(""));
        assert_eq!(result.value(0, 2), Some("NULL"));
        assert_eq!(result.rows[0][0].capacity(), 0);
        for _ in 0..100 {
            result.push_row(vec![Some("x".into()), None, Some("NULL".into())]);
        }
        assert!(result.is_null(100, 1));
        assert!(!result.is_null(100, 2));
        assert!(!result.is_null(usize::MAX, 1));
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["rows"][0], serde_json::json!([null, "", "NULL"]));
        assert_eq!(result.null_cells.len(), 5);
    }
}
