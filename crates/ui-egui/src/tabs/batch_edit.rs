//! One shared input buffer and physical cell addresses; row data stays untouched until commit.
pub const LIMIT: usize = 8 * 1024 * 1024;
pub struct Batch {
    pub cells: Vec<(usize, usize)>,
    pub text: String,
    committed: usize,
    pub composing: bool,
}
pub enum Edit<'a> {
    Text(&'a str),
    Preedit(&'a str),
    Commit(&'a str),
    Backspace,
}
impl Batch {
    pub fn new(cells: impl Iterator<Item = (usize, usize)>) -> anyhow::Result<Option<Self>> {
        let mut cells: Vec<_> = cells.take(65_537).collect();
        anyhow::ensure!(cells.len() <= 65_536, "批量编辑最多选择 65,536 个单元格");
        cells.sort_unstable();
        cells.dedup();
        anyhow::ensure!(
            cells
                .windows(2)
                .filter(|cells| cells[0].0 != cells[1].0)
                .count()
                + usize::from(!cells.is_empty())
                <= 1000,
            "批量编辑最多选择 1,000 行"
        );
        Ok((cells.len() >= 2).then_some(Self {
            cells,
            text: String::new(),
            committed: 0,
            composing: false,
        }))
    }
    pub fn value(&self, row: usize, column: usize) -> Option<&str> {
        self.cells
            .binary_search(&(row, column))
            .ok()
            .map(|_| self.text.as_str())
    }
    pub fn edit(&mut self, edit: Edit<'_>) -> anyhow::Result<()> {
        let (prefix, suffix, composing) = match edit {
            Edit::Text(text) | Edit::Commit(text) => (self.committed, text, false),
            Edit::Preedit(text) => (self.committed, text, !text.is_empty()),
            Edit::Backspace => (
                self.text[..self.committed]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(index, _)| index),
                "",
                false,
            ),
        };
        anyhow::ensure!(
            (prefix + suffix.len()).saturating_mul(self.cells.len()) <= LIMIT,
            "批量编辑内容超过 8 MiB，请缩小选区"
        );
        self.text.truncate(prefix);
        self.text.push_str(suffix);
        self.composing = composing;
        if !composing {
            self.committed = self.text.len();
        }
        Ok(())
    }
    pub fn preview(&self, row: usize, column: usize) -> Option<&str> {
        self.value(row, column).map(|text| {
            &text[..text
                .char_indices()
                .nth(160)
                .map_or(text.len(), |(index, _)| index)]
        })
    }
}
