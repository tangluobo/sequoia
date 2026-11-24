//! Real-server parity between legacy complete and two-phase object APIs.
use super::*;
use std::sync::Arc;
use tomato_core::database_objects::Catalog;

pub(super) async fn verify(
    db: &mut Databases,
    connection: &Connection,
    schema: &str,
    complete: &Catalog,
) -> Result<()> {
    let DatabaseOutput::ObjectHeaders(headers) = db
        .workbench(
            connection,
            DatabaseAction::ObjectHeaders {
                schema: schema.into(),
            },
        )
        .await?
    else {
        anyhow::bail!("header output required")
    };
    assert!(
        headers
            .objects
            .iter()
            .all(|object| object.fields.is_empty())
    );
    let mut expected = serde_json::to_value(complete)?;
    for object in expected["objects"].as_array_mut().unwrap() {
        object["fields"] = serde_json::json!([]);
    }
    assert_eq!(
        serde_json::to_value(&headers)?,
        expected,
        "header properties/FKs changed between APIs"
    );
    let headers = Arc::new(headers);
    let DatabaseOutput::ObjectFields {
        schema: returned,
        fields,
    } = db
        .workbench(
            connection,
            DatabaseAction::ObjectFields {
                schema: schema.into(),
                catalog: headers.clone(),
            },
        )
        .await?
    else {
        anyhow::bail!("field output required")
    };
    assert_eq!(returned, schema);
    assert!(
        headers
            .objects
            .iter()
            .all(|object| object.fields.is_empty()),
        "preload mutated its immutable input"
    );
    let mut names = std::collections::BTreeSet::new();
    for table in &fields {
        assert!(
            names.insert(table.name.as_str()),
            "duplicate exact table name"
        );
    }
    for object in &complete.objects {
        let actual = fields
            .iter()
            .find(|table| table.name == object.table.name)
            .map(|table| table.columns.as_slice())
            .unwrap_or_default();
        assert_eq!(
            serde_json::to_value(actual)?,
            serde_json::to_value(&object.fields)?,
            "field order/name/comment changed for {}",
            object.table.name
        );
    }
    assert!(
        fields.iter().all(|table| complete
            .objects
            .iter()
            .any(|object| object.table.name == table.name)),
        "background preload returned an unrelated table"
    );
    Ok(())
}
