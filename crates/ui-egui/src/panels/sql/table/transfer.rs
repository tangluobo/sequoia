use super::*;

pub(super) fn copy(state: &Workbench, kind: &str, format: u8) -> anyhow::Result<String> {
    let Some(page) = &state.page else {
        return Ok(String::new());
    };
    let rows: Vec<_> = page
        .rows
        .iter()
        .enumerate()
        .filter(|(index, _)| state.selected.is_empty() || state.selected.contains(index))
        .map(|(index, row)| state.edits.get(&index).unwrap_or(row).clone())
        .collect();
    match format {
        0 => tomato_services::database::inserts_sql(kind, &page.definition, &rows),
        1 => {
            let rows: Vec<_> = rows
                .iter()
                .map(|row| {
                    page.definition
                        .columns
                        .iter()
                        .zip(row)
                        .map(|(column, value)| {
                            let value = match value {
                                Cell::Null => serde_json::Value::Null,
                                Cell::Text(value) => serde_json::Value::String(value.clone()),
                                Cell::Integer(value) => (*value).into(),
                                Cell::Real(value) => serde_json::json!(value),
                                Cell::Boolean(value) => (*value).into(),
                                Cell::Binary(_) => value.display().into(),
                            };
                            (column.name.clone(), value)
                        })
                        .collect::<serde_json::Map<_, _>>()
                })
                .collect();
            Ok(serde_json::to_string_pretty(&rows)?)
        }
        _ => Ok(rows
            .iter()
            .map(|row| row.iter().map(Cell::display).collect::<Vec<_>>().join("\t"))
            .collect::<Vec<_>>()
            .join("\n")),
    }
}

pub(super) fn formatted(
    state: &Workbench,
    kind: context_menu::Copy,
    all: bool,
) -> anyhow::Result<String> {
    use std::collections::{BTreeMap, BTreeSet};
    let Some(page) = &state.page else {
        return Ok(String::new());
    };
    if matches!(kind, context_menu::Copy::Plain) {
        let visible = projection::rows(state);
        let bounds = visible
            .iter()
            .enumerate()
            .filter(|(_, row)| state.table_view.cells.row_selected(**row))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut result = String::new();
        if let (Some(&first), Some(&last)) = (bounds.first(), bounds.last()) {
            for (index, &row) in visible[first..=last].iter().enumerate() {
                if index > 0 {
                    result.push('\n');
                }
                let mut first_column = true;
                for column in 0..page.definition.columns.len() {
                    if !state.table_view.cells.contains(&(row, column)) {
                        continue;
                    }
                    if !first_column {
                        result.push('\t');
                    }
                    first_column = false;
                    if let Some(value) = projection::text(state, row, column) {
                        anyhow::ensure!(
                            result.len().saturating_add(value.len()) <= 8 * 1024 * 1024,
                            "复制或导出内容超过 8 MiB，请缩小范围"
                        );
                        result.push_str(&value);
                    }
                    anyhow::ensure!(
                        result.len() <= 8 * 1024 * 1024,
                        "复制或导出内容超过 8 MiB，请缩小范围"
                    );
                }
            }
        }
        return Ok(result);
    }
    let visible = projection::rows(state);
    let selected_rows: BTreeSet<_> = state.table_view.cells.iter().map(|cell| cell.0).collect();
    let rows: Vec<_> = visible
        .into_iter()
        .filter(|row| all || selected_rows.contains(row))
        .collect();
    let columns: Vec<_> = if all {
        (0..page.definition.columns.len()).collect()
    } else {
        state
            .table_view
            .cells
            .iter()
            .map(|cell| cell.1)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    };
    let names: Vec<_> = page
        .definition
        .columns
        .iter()
        .map(|column| column.name.clone())
        .collect();
    let mut display = BTreeMap::new();
    let mut size = 0usize;
    for &row in &rows {
        for &column in &columns {
            if let Some(value) = projection::text(state, row, column) {
                size = size.saturating_add(value.len());
                anyhow::ensure!(
                    size <= 8 * 1024 * 1024,
                    "复制或导出内容超过 8 MiB，请缩小范围"
                );
                if let std::borrow::Cow::Owned(value) = value {
                    display.insert((row, column), value);
                }
            }
        }
    }
    let keys: Vec<_> = page
        .definition
        .columns
        .iter()
        .filter(|column| column.primary_key > 0)
        .map(|column| column.name.clone())
        .collect();
    // UPDATE predicates can reference an unselected primary key without exporting extra fields.
    if matches!(kind, context_menu::Copy::Update) {
        for &row in &rows {
            for (column, _) in page
                .definition
                .columns
                .iter()
                .enumerate()
                .filter(|(_, field)| field.primary_key > 0)
            {
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    display.entry((row, column))
                    && let Some(std::borrow::Cow::Owned(value)) =
                        projection::text(state, row, column)
                {
                    size = size.saturating_add(value.len());
                    anyhow::ensure!(
                        size <= 8 * 1024 * 1024,
                        "复制或导出内容超过 8 MiB，请缩小范围"
                    );
                    entry.insert(value);
                }
            }
        }
    }
    let table = if page.definition.table.schema.is_empty() {
        page.definition.table.name.clone()
    } else {
        format!(
            "{}.{}",
            page.definition.table.schema, page.definition.table.name
        )
    };
    tomato_core::result_transfer::format(
        tomato_core::result_transfer::Area {
            columns: &names,
            rows: &rows,
            selected_columns: &columns,
            value: |row, column| {
                state
                    .batch
                    .as_ref()
                    .and_then(|batch| batch.value(row, column))
                    .or_else(|| match projection::value(state, row, column) {
                        Some(Cell::Text(text)) => Some(text.as_str()),
                        _ => display.get(&(row, column)).map(String::as_str),
                    })
            },
        },
        context_menu::format(kind, &table, &keys),
        &page.definition.comments,
    )
}
