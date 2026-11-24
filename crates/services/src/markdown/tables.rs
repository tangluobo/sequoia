//! The Java preview and HTML writer recognize the same line-based tables.
use std::borrow::Cow;

pub(super) const MAX_COLUMNS: usize = 4096;

pub(super) enum Segment<'a> {
    Text(&'a str),
    Table(Table<'a>),
}

pub(super) struct Table<'a> {
    pub header: &'a str,
    pub divider: &'a str,
    pub body: &'a str,
}

pub(super) fn segments(source: &str) -> impl Iterator<Item = Segment<'_>> {
    let mut lines = source.split_inclusive('\n').peekable();
    let mut offset = 0;
    let mut text_start = 0;
    let mut pending = None;
    std::iter::from_fn(move || {
        if let Some(table) = pending.take() {
            return Some(Segment::Table(table));
        }
        while let Some(header) = lines.next() {
            let header_start = offset;
            offset += header.len();
            if !header.contains('|') {
                continue;
            }
            let Some(divider) = lines.next_if(|line| is_delimiter(line)) else {
                continue;
            };
            offset += divider.len();
            let body_start = offset;
            while let Some(row) = lines.next_if(|line| line.contains('|')) {
                offset += row.len();
            }
            let table = Table {
                header,
                divider,
                body: &source[body_start..offset],
            };
            let prose = &source[text_start..header_start];
            text_start = offset;
            if prose.is_empty() {
                return Some(Segment::Table(table));
            }
            pending = Some(table);
            return Some(Segment::Text(prose.strip_suffix('\n').unwrap_or(prose)));
        }
        let prose = &source[text_start..];
        text_start = source.len();
        (!prose.is_empty()).then_some(Segment::Text(prose))
    })
}

fn is_delimiter(line: &str) -> bool {
    const SPACE: [char; 6] = [' ', '\t', '\n', '\r', '\u{b}', '\u{c}'];
    let line = line.trim_matches(SPACE);
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').all(|cell| {
        let cell = cell.trim_matches(SPACE);
        let cell = cell.strip_prefix(':').unwrap_or(cell);
        let cell = cell.strip_suffix(':').unwrap_or(cell);
        !cell.is_empty() && cell.bytes().all(|byte| byte == b'-')
    })
}

pub(super) fn cells(line: &str) -> impl Iterator<Item = Cow<'_, str>> {
    let line = super::resource::java_trim(line);
    let line = line.strip_prefix('|').unwrap_or(line);
    let mut remaining = Some(if line.ends_with(r"\|") {
        line
    } else {
        line.strip_suffix('|').unwrap_or(line)
    });
    std::iter::from_fn(move || {
        let line = remaining.take()?;
        let separator = line
            .as_bytes()
            .iter()
            .enumerate()
            .find_map(|(index, byte)| {
                (*byte == b'|' && (index == 0 || line.as_bytes()[index - 1] != b'\\'))
                    .then_some(index)
            });
        let value = if let Some(index) = separator {
            remaining = Some(&line[index + 1..]);
            &line[..index]
        } else {
            line
        };
        let value = super::resource::java_trim(value);
        Some(if value.contains(r"\|") {
            Cow::Owned(value.replace(r"\|", "|"))
        } else {
            Cow::Borrowed(value)
        })
    })
}
