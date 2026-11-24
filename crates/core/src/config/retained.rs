use super::*;
pub fn value(value: &Value) -> usize {
    match value {
        Value::String(text) => text.capacity(),
        Value::Array(values) => values.iter().fold(
            values.capacity() * std::mem::size_of::<Value>(),
            |sum, item| sum.saturating_add(self::value(item)),
        ),
        Value::Object(values) => values.iter().fold(0usize, |sum, (key, item)| {
            sum.saturating_add(key.capacity())
                .saturating_add(self::value(item))
                .saturating_add(64)
        }),
        _ => std::mem::size_of::<Value>(),
    }
}
impl Connection {
    pub fn retained_bytes(&self) -> usize {
        let base = std::mem::size_of::<Self>()
            + self.id.capacity()
            + self.name.capacity()
            + self.parent_id.as_ref().map_or(0, String::capacity)
            + self.kind.as_ref().map_or(0, String::capacity);
        self.options.iter().fold(base, |sum, (key, item)| {
            sum.saturating_add(key.capacity())
                .saturating_add(value(item))
                .saturating_add(64)
        })
    }
}
