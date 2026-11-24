use std::iter::{Extend, IntoIterator};
use std::sync::Arc;
use crate::MySqlColumn;

#[derive(Debug, Default)]
pub struct MySqlQueryResult {
    pub(super) rows_affected: u64,
    pub(super) last_insert_id: u64,
    pub(crate) columns: Option<Arc<Vec<MySqlColumn>>>,
}

impl MySqlQueryResult {
    pub fn has_result_set(&self) -> bool {self.columns.is_some()}
    /// Result-set columns, including an empty SELECT's column metadata.
    pub fn columns(&self) -> &[MySqlColumn] {self.columns.as_deref().map_or(&[], Vec::as_slice)}
    pub fn last_insert_id(&self) -> u64 {
        self.last_insert_id
    }

    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

impl Extend<MySqlQueryResult> for MySqlQueryResult {
    fn extend<T: IntoIterator<Item = MySqlQueryResult>>(&mut self, iter: T) {
        for elem in iter {
            self.rows_affected += elem.rows_affected;
            self.last_insert_id = elem.last_insert_id;
            self.columns = elem.columns;
        }
    }
}
#[cfg(feature = "any")]
/// This conversion attempts to save last_insert_id by converting to i64.
impl From<MySqlQueryResult> for sqlx_core::any::AnyQueryResult {
    fn from(done: MySqlQueryResult) -> Self {
        sqlx_core::any::AnyQueryResult {
            rows_affected: done.rows_affected(),
            last_insert_id: done.last_insert_id().try_into().ok(),
        }
    }
}
