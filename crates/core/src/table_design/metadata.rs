use super::Dialect;
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct Charset {
    pub name: String,
    pub collations: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Metadata {
    pub version: Option<String>,
    pub backslash_escapes: bool,
    pub data_types: &'static [&'static str],
    pub engines: Vec<String>,
    pub charsets: Vec<Charset>,
    pub warnings: Vec<String>,
}
impl Metadata {
    pub fn empty(dialect: Dialect) -> Self {
        Self {
            version: None,
            backslash_escapes: false,
            data_types: super::data_types(dialect, None),
            engines: Vec::new(),
            charsets: Vec::new(),
            warnings: Vec::new(),
        }
    }
    pub fn retained_bytes(&self) -> usize {
        let strings = |values: &Vec<String>| {
            values.capacity() * std::mem::size_of::<String>()
                + values.iter().map(String::capacity).sum::<usize>()
        };
        std::mem::size_of::<Self>()
            + self.version.as_ref().map_or(0, String::capacity)
            + strings(&self.engines)
            + strings(&self.warnings)
            + self.charsets.capacity() * std::mem::size_of::<Charset>()
            + self
                .charsets
                .iter()
                .map(|charset| charset.name.capacity() + strings(&charset.collations))
                .sum::<usize>()
    }
}
