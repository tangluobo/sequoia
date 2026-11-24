use std::iter::{Extend, IntoIterator};
use std::sync::Arc;
use crate::{PgColumn,statement::PgStatementMetadata};

#[derive(Debug, Default)]
pub struct PgQueryResult {
    pub(super) rows_affected: u64,
    pub(crate) metadata: Option<Arc<PgStatementMetadata>>,
}

impl PgQueryResult {
    /// Whether the command returned a result set, including an empty SELECT.
    pub fn has_result_set(&self) -> bool {self.metadata.is_some()}
    pub fn columns(&self) -> &[PgColumn] {self.metadata.as_ref().map_or(&[],|metadata|metadata.columns.as_slice())}
    pub fn rows_affected(&self) -> u64 {
        self.rows_affected
    }
}

impl Extend<PgQueryResult> for PgQueryResult {
    fn extend<T: IntoIterator<Item = PgQueryResult>>(&mut self, iter: T) {
        for elem in iter {
            self.rows_affected += elem.rows_affected;
            self.metadata = elem.metadata;
        }
    }
}

#[cfg(feature = "any")]
impl From<PgQueryResult> for sqlx_core::any::AnyQueryResult {
    fn from(done: PgQueryResult) -> Self {
        sqlx_core::any::AnyQueryResult {
            rows_affected: done.rows_affected,
            last_insert_id: None,
        }
    }
}
