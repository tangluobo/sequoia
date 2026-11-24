use crate::ext::ustr::UStr;
use crate::protocol::text::ColumnFlags;
use crate::{MySql, MySqlTypeInfo};
use std::sync::Arc;
pub(crate) use sqlx_core::column::*;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "offline", derive(serde::Serialize, serde::Deserialize))]
pub struct MySqlColumn {
    pub(crate) ordinal: usize,
    pub(crate) name: UStr,
    pub(crate) type_info: MySqlTypeInfo,

    #[cfg_attr(feature = "offline", serde(skip))]
    pub(crate) flags: Option<ColumnFlags>,
    #[cfg_attr(feature = "offline", serde(skip))]
    pub(crate) origin: Option<Arc<ColumnOrigin>>,
}
#[derive(Debug, Clone)]
pub(crate) struct ColumnOrigin {pub(crate) database:UStr,pub(crate) table:UStr,pub(crate) name:UStr}
impl MySqlColumn {
    /// Original database/table/column names, absent for expressions or offline metadata.
    pub fn source_database(&self) -> Option<&str> {self.origin.as_ref().map(|origin|&*origin.database)}
    pub fn source_table(&self) -> Option<&str> {self.origin.as_ref().map(|origin|&*origin.table)}
    pub fn source_name(&self) -> Option<&str> {self.origin.as_ref().map(|origin|&*origin.name)}
}

impl Column for MySqlColumn {
    type Database = MySql;

    fn ordinal(&self) -> usize {
        self.ordinal
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn type_info(&self) -> &MySqlTypeInfo {
        &self.type_info
    }
}
