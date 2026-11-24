//! Serialize a shared request using the ordinary catalog representation.
//! In-process commands keep one snapshot instead of copying its header strings.
use super::Catalog;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

pub(crate) fn serialize<S: Serializer>(value: &Arc<Catalog>, output: S) -> Result<S::Ok, S::Error> {
    value.as_ref().serialize(output)
}
pub(crate) fn deserialize<'de, D: Deserializer<'de>>(input: D) -> Result<Arc<Catalog>, D::Error> {
    Catalog::deserialize(input).map(Arc::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workbench::DatabaseAction;
    #[test]
    fn shared_requests_clone_the_snapshot_and_roundtrip_the_ordinary_catalog_json() {
        let catalog = Arc::new(Catalog::default());
        let action = DatabaseAction::ObjectFields {
            schema: "scope".into(),
            catalog: catalog.clone(),
        };
        let DatabaseAction::ObjectFields {
            catalog: cloned, ..
        } = action.clone()
        else {
            unreachable!()
        };
        assert!(Arc::ptr_eq(&catalog, &cloned));
        let text = serde_json::to_string(&action).unwrap();
        let DatabaseAction::ObjectFields {
            schema,
            catalog: restored,
        } = serde_json::from_str(&text).unwrap()
        else {
            unreachable!()
        };
        assert_eq!(schema, "scope");
        assert_eq!(
            serde_json::to_value(&*catalog).unwrap(),
            serde_json::to_value(&*restored).unwrap()
        );
    }
}
