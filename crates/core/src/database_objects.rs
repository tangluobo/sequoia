//! Object browser metadata, independent of SQL drivers and presentation widgets.
use crate::workbench::TableId;
use serde::{Deserialize, Serialize};
mod actions;
pub(crate) mod shared_catalog;
pub use actions::{ChangeReport, Mutation, Request, Target};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub comment: String,
}

/// Owned field vectors travel independently of object headers during preload.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fields<T = Field> {
    pub name: String,
    pub columns: Vec<T>,
}

pub fn field_bytes(fields: &Vec<Field>) -> usize {
    fields.capacity() * std::mem::size_of::<Field>()
        + fields
            .iter()
            .map(|f| f.name.capacity() + f.comment.capacity())
            .sum::<usize>()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Object {
    pub table: TableId,
    pub view: bool,
    pub comment: String,
    pub engine: String,
    pub auto_increment: String,
    pub update_time: String,
    pub data_length: String,
    pub rows: String,
    #[serde(default)]
    pub fields: Vec<Field>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForeignKey {
    pub name: String,
    pub table: TableId,
    pub columns: Vec<String>,
    pub referenced: TableId,
    pub referenced_columns: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog {
    pub objects: Vec<Object>,
    pub foreign_keys: Vec<ForeignKey>,
}
impl Catalog {
    /// Shared metadata is accounted once per queue request, without serializing
    /// or cloning it. Vector spare capacity also belongs to the snapshot.
    pub fn retained_bytes(&self) -> usize {
        let mut bytes = std::mem::size_of::<Self>()
            + self.objects.capacity() * std::mem::size_of::<Object>()
            + self.foreign_keys.capacity() * std::mem::size_of::<ForeignKey>();
        for object in &self.objects {
            bytes = bytes.saturating_add(
                object.table.schema.capacity()
                    + object.table.name.capacity()
                    + object.comment.capacity()
                    + object.engine.capacity()
                    + object.auto_increment.capacity()
                    + object.update_time.capacity()
                    + object.data_length.capacity()
                    + object.rows.capacity()
                    + field_bytes(&object.fields),
            );
        }
        for key in &self.foreign_keys {
            bytes = bytes.saturating_add(
                key.name.capacity()
                    + key.table.schema.capacity()
                    + key.table.name.capacity()
                    + key.referenced.schema.capacity()
                    + key.referenced.name.capacity()
                    + (key.columns.capacity() + key.referenced_columns.capacity())
                        * std::mem::size_of::<String>()
                    + key
                        .columns
                        .iter()
                        .chain(&key.referenced_columns)
                        .map(String::capacity)
                        .sum::<usize>(),
            );
        }
        bytes
    }
}

/// Java DatabaseService.formatDataLength uses base 1024 and two decimal places.
pub fn data_length(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut divisor = 1024_u128;
    for unit in ["KB", "MB", "GB"] {
        if u128::from(bytes) < divisor * 1024 {
            return formatted(bytes, divisor, unit);
        }
        divisor *= 1024;
    }
    formatted(bytes, divisor, "TB")
}
fn formatted(bytes: u64, divisor: u128, unit: &str) -> String {
    let hundredths = (u128::from(bytes) * 100 + divisor / 2) / divisor;
    format!("{}.{:02} {unit}", hundredths / 100, hundredths % 100)
}
#[cfg(test)]
mod tests {
    #[test]
    fn object_sizes_match_original_java_including_half_up_boundaries() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../tests/fixtures/objects/original-sizes.json"
        ))
        .unwrap();
        for case in cases.as_array().unwrap() {
            assert_eq!(
                super::data_length(case["bytes"].as_u64().unwrap()),
                case["display"].as_str().unwrap()
            );
        }
        assert_eq!(super::data_length(0), "");
    }
}
