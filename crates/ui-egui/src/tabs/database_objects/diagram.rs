//! ER positions and links are built on metadata/filter changes, never per frame.
use tomato_core::database_objects::Catalog;
pub struct Diagram {
    pub tables: Vec<usize>,
    pub links: Vec<(usize, usize, usize)>,
    pub columns: usize,
}
impl Diagram {
    pub fn new(catalog: &Catalog, filtered: &[usize]) -> Self {
        let tables: Vec<_> = filtered
            .iter()
            .copied()
            .filter(|index| !catalog.objects[*index].view)
            .collect();
        let columns = (tables.len() as f64).sqrt().ceil().max(1.) as usize;
        // Sort borrowed indices for lookup, retaining no duplicate object names.
        let mut by_name: Vec<_> = (0..tables.len()).collect();
        by_name.sort_unstable_by(|a, b| {
            catalog.objects[tables[*a]]
                .table
                .name
                .cmp(&catalog.objects[tables[*b]].table.name)
        });
        let position = |name: &str| {
            by_name
                .binary_search_by(|index| {
                    catalog.objects[tables[*index]]
                        .table
                        .name
                        .as_str()
                        .cmp(name)
                })
                .ok()
                .map(|found| by_name[found])
        };
        let links = catalog
            .foreign_keys
            .iter()
            .enumerate()
            .filter_map(|(index, key)| {
                if key.table.schema != key.referenced.schema {
                    return None;
                }
                Some((
                    position(&key.table.name)?,
                    position(&key.referenced.name)?,
                    index,
                ))
            })
            .collect();
        Self {
            tables,
            links,
            columns,
        }
    }
    pub fn rect(&self, origin: egui::Pos2, ordinal: usize) -> egui::Rect {
        egui::Rect::from_min_size(
            origin
                + egui::vec2(
                    20. + (ordinal % self.columns) as f32 * 280.,
                    20. + (ordinal / self.columns) as f32 * 180.,
                ),
            egui::vec2(182., 25.),
        )
    }
}
