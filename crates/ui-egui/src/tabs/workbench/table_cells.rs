//! Allocate one bit per selected field, and no storage for unselected rows.
use std::collections::BTreeMap;
#[derive(Default)]
pub struct Cells(BTreeMap<usize, Vec<u64>>);
impl Cells {
    pub fn retain_rows(&mut self, visible: &std::collections::BTreeSet<usize>) {
        self.0.retain(|row, _| visible.contains(row));
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn contains(&self, &(row, col): &(usize, usize)) -> bool {
        self.0
            .get(&row)
            .and_then(|bits| bits.get(col / 64))
            .is_some_and(|bits| bits & (1 << (col % 64)) != 0)
    }
    pub fn insert(&mut self, (row, col): (usize, usize)) {
        let bits = self.0.entry(row).or_default();
        bits.resize(bits.len().max(col / 64 + 1), 0);
        bits[col / 64] |= 1 << (col % 64);
    }
    pub fn remove(&mut self, &(row, col): &(usize, usize)) {
        if let Some(bits) = self.0.get_mut(&row) {
            if let Some(value) = bits.get_mut(col / 64) {
                *value &= !(1 << (col % 64));
            }
            if bits.iter().all(|value| *value == 0) {
                self.0.remove(&row);
            }
        }
    }
    pub fn row_selected(&self, row: usize) -> bool {
        self.0.contains_key(&row)
    }
    pub fn iter(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.0.iter().flat_map(|(&row, bits)| {
            bits.iter().enumerate().flat_map(move |(word, &value)| {
                (0..64)
                    .filter(move |bit| value & (1 << bit) != 0)
                    .map(move |bit| (row, word * 64 + bit))
            })
        })
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.0
            .values()
            .flatten()
            .map(|value| value.count_ones() as usize)
            .sum()
    }
}
