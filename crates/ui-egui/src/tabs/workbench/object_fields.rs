//! Move background field vectors into the published snapshot without resetting
//! editors, sort widths, selection or scroll when no search must be reapplied.
use super::*;
use anyhow::{Result, ensure};
use tomato_core::database_objects::{Catalog, Fields, field_bytes};
#[cfg(test)]
mod tests;

impl Workbench {
    pub(super) fn receive_object_catalog(&mut self, catalog: Catalog, headers_only: bool) {
        let catalog = Arc::new(catalog);
        if let Some(browser) = &mut self.browser {
            if headers_only {
                browser.receive_headers(catalog.clone());
            } else {
                browser.receive_shared(catalog.clone());
            }
        }
        self.objects = Some(catalog);
    }
    pub(crate) fn receive_object_fields(&mut self, fields: Vec<Fields>) -> Result<()> {
        let browser = self
            .browser
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("对象标签已关闭"))?;
        ensure!(fields.len() <= 10_000, "对象字段超出范围，请刷新");
        let mut names: Vec<_> = (0..browser.catalog.objects.len()).collect();
        names.sort_unstable_by(|&a, &b| {
            browser.catalog.objects[a]
                .table
                .name
                .cmp(&browser.catalog.objects[b].table.name)
        });
        let mut projected = browser.catalog.retained_bytes();
        let mut payload_order: Vec<_> = (0..fields.len()).collect();
        payload_order.sort_unstable_by(|&a, &b| fields[a].name.cmp(&fields[b].name));
        ensure!(
            payload_order
                .windows(2)
                .all(|pair| fields[pair[0]].name != fields[pair[1]].name),
            "服务器返回了重复的对象字段"
        );
        let scratch = (names.capacity() + payload_order.capacity()) * std::mem::size_of::<usize>();
        ensure!(
            projected.saturating_add(scratch) <= tomato_core::limits::QUERY_BYTES,
            "对象目录超过 8 MiB，请缩小数据库范围"
        );
        let mut count = 0usize;
        for table in &fields {
            count = count.saturating_add(table.columns.len());
            ensure!(count <= 100_000, "对象字段超出范围，请刷新");
            let Ok(at) =
                names.binary_search_by(|&i| browser.catalog.objects[i].table.name.cmp(&table.name))
            else {
                continue;
            };
            let object = &browser.catalog.objects[names[at]];
            projected = projected
                .saturating_sub(field_bytes(&object.fields))
                .saturating_add(field_bytes(&table.columns));
            ensure!(
                projected.saturating_add(scratch) <= tomato_core::limits::QUERY_BYTES,
                "对象目录超过 8 MiB，请缩小数据库范围"
            );
        }
        drop(payload_order);
        // These are the usual two owners. Drop the Workbench alias before
        // unwrapping; do not copy every header for an ordinary preload.
        self.objects = None;
        let shared = std::mem::take(&mut browser.state.catalog);
        // An explicitly retained immutable external snapshot keeps its old
        // contents. Only that uncommon case requires a bounded header copy.
        let mut catalog = Arc::unwrap_or_clone(shared);
        for table in fields {
            if let Ok(at) =
                names.binary_search_by(|&i| catalog.objects[i].table.name.cmp(&table.name))
            {
                catalog.objects[names[at]].fields = table.columns;
            }
        }
        let catalog = Arc::new(catalog);
        browser.state.catalog = catalog.clone();
        self.objects = Some(catalog);
        browser.fields_loading = super::super::database_objects::window::FieldsLoading::Ready;
        browser.state.fields_pending = false;
        if !browser.search.trim_matches(|c: char| c <= ' ').is_empty() {
            // Java reapplies current nonempty search even before its queued
            // debounce fires; the ordinary filter path owns selection rules.
            browser.filter();
        }
        Ok(())
    }
}
