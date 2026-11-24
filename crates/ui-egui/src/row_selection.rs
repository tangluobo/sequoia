//! Shared row selection; identity snapshots stay in feature controllers.
#[derive(Default)]
pub struct Selection {
    pub rows: std::collections::BTreeSet<usize>,
    anchor: Option<usize>,
}
impl Selection {
    pub fn clear(&mut self) {
        self.rows.clear();
        self.anchor = None;
    }
    pub fn select(
        &mut self,
        row: usize,
        active: &mut Option<usize>,
        modifiers: egui::Modifiers,
        secondary: bool,
    ) {
        if secondary && self.rows.contains(&row) {
            *active = Some(row);
            return;
        }
        let extend = modifiers.command || modifiers.ctrl;
        if modifiers.shift && !secondary {
            let anchor = self.anchor.or(*active).unwrap_or(row);
            if !extend {
                self.rows.clear();
            }
            self.rows.extend(anchor.min(row)..=anchor.max(row));
        } else if extend && !secondary {
            if !self.rows.insert(row) {
                self.rows.remove(&row);
            }
            self.anchor = Some(row);
        } else {
            self.rows = [row].into();
            self.anchor = Some(row);
        }
        *active = if self.rows.contains(&row) {
            Some(row)
        } else {
            self.rows.last().copied()
        };
    }
}
