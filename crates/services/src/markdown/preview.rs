//! Preserve the Java preview's block and cell boundaries before laying out text.
use super::{Block, Document, Image, Kind, MAX_SOURCE_BYTES, Span, Style, tables};
use anyhow::{Result, ensure};
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

const MAX_CELLS: usize = 48_000;
const BLOCK_GAP: u8 = 6;
const LIST_GAP: u8 = 2;

pub fn parse(source: &str) -> Result<Document> {
    ensure!(source.len() <= MAX_SOURCE_BYTES, "Markdown 最多 8 MiB");
    let mut preview = Preview {
        document: Document::default(),
        events_left: 250_000,
        cells_left: MAX_CELLS,
    };
    for segment in tables::segments(source) {
        match segment {
            tables::Segment::Text(prose) => preview.prose(prose)?,
            tables::Segment::Table(table) => preview.table(table)?,
        }
    }
    Ok(preview.document)
}

struct Preview {
    document: Document,
    events_left: usize,
    cells_left: usize,
}

impl Preview {
    fn event(&mut self) -> Result<()> {
        ensure!(self.events_left != 0, "Markdown 结构超过预览上限");
        self.events_left -= 1;
        Ok(())
    }

    fn push(&mut self, block: Block) -> Result<()> {
        ensure!(
            self.document.blocks.len() < 20_000,
            "Markdown 结构超过预览上限"
        );
        self.document.blocks.push(block);
        Ok(())
    }

    fn flush(&mut self, block: &mut Block, in_list: bool) -> Result<()> {
        let kind = std::mem::replace(&mut block.kind, Kind::Paragraph);
        let indent = std::mem::take(&mut block.indent);
        if !block.spans.is_empty() || matches!(kind, Kind::Rule) {
            self.push(Block {
                kind,
                spans: std::mem::take(&mut block.spans),
                indent,
                quote: block.quote,
                gap: list_gap(in_list),
            })?;
        }
        Ok(())
    }

    fn prose(&mut self, source: &str) -> Result<()> {
        let mut current = Block::new(Kind::Paragraph);
        let mut inline = Inline::default();
        let mut lists = Vec::<Option<u64>>::new();
        let mut quote = 0usize;
        let mut marker_only = false;
        for event in Parser::new(source) {
            self.event()?;
            if inline.handling_image() || is_inline(&event) {
                let count = current.spans.len();
                inline.push(event, &mut current.spans)?;
                marker_only &= count == current.spans.len();
                continue;
            }
            let in_list = !lists.is_empty();
            match event {
                Event::Start(Tag::Paragraph) => current.indent = usize::from(in_list),
                Event::Start(Tag::Heading { level, .. }) if !marker_only => {
                    self.flush(&mut current, in_list)?;
                    current.kind = Kind::Heading(level as u8);
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    self.flush(&mut current, in_list)?;
                    current.kind = Kind::Code(match kind {
                        CodeBlockKind::Fenced(language) => language.into_string(),
                        _ => String::new(),
                    });
                }
                Event::Start(Tag::HtmlBlock) => {
                    self.flush(&mut current, in_list)?;
                    current.kind = Kind::HtmlBlock;
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    self.flush(&mut current, in_list)?;
                    quote += 1;
                    current.quote = true;
                }
                Event::Start(Tag::List(start)) => {
                    self.flush(&mut current, in_list)?;
                    ensure!(lists.len() < 64, "Markdown 列表嵌套超过 64 层");
                    lists.push(start);
                }
                Event::Start(Tag::Item) => {
                    self.flush(&mut current, in_list)?;
                    current.indent = 1;
                    let text = if let Some(Some(number)) = lists.last_mut() {
                        let text = format!("{number}. ");
                        *number = number.saturating_add(1);
                        text
                    } else {
                        "• ".into()
                    };
                    current.spans.push(Span::plain(text));
                    marker_only = true;
                    continue;
                }
                Event::End(
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::CodeBlock
                    | TagEnd::HtmlBlock
                    | TagEnd::Item,
                ) => {
                    self.flush(&mut current, in_list)?;
                }
                Event::End(TagEnd::List(_)) => {
                    self.flush(&mut current, in_list)?;
                    lists.pop();
                    if let Some(last) = self.document.blocks.last_mut() {
                        last.gap = list_gap(!lists.is_empty());
                    }
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    self.flush(&mut current, in_list)?;
                    quote = quote.saturating_sub(1);
                    current.quote = quote != 0;
                }
                Event::Rule => {
                    self.flush(&mut current, in_list)?;
                    current.kind = Kind::Rule;
                    self.flush(&mut current, in_list)?;
                }
                _ => {}
            }
            marker_only &= !current.spans.is_empty();
        }
        self.flush(&mut current, false)
    }

    fn table(&mut self, table: tables::Table<'_>) -> Result<()> {
        let columns = tables::cells(table.header)
            .take(tables::MAX_COLUMNS + 1)
            .count();
        ensure!(columns <= tables::MAX_COLUMNS, "Markdown 表格超过 4096 列");
        let mut rows = Vec::new();
        for line in std::iter::once(table.header).chain(table.body.split_inclusive('\n')) {
            ensure!(
                columns <= self.cells_left,
                "Markdown 表格超过 48000 个预览单元格"
            );
            self.cells_left -= columns;
            let mut cells = tables::cells(line);
            let mut row = Vec::with_capacity(columns);
            for _ in 0..columns {
                row.push(match cells.next() {
                    Some(cell) => self.cell(&cell)?,
                    None => Vec::new(),
                });
            }
            rows.push(row);
        }
        self.push(Block::new(Kind::Table(rows)))
    }

    fn cell(&mut self, text: &str) -> Result<Vec<Span>> {
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let mut events = Parser::new(text);
        if !matches!(events.next(), Some(Event::Start(Tag::Paragraph))) {
            return Ok(vec![Span::plain(text.to_owned())]);
        }
        let mut inline = Inline::default();
        let mut spans = Vec::new();
        for event in events.take_while(|event| !matches!(event, Event::End(TagEnd::Paragraph))) {
            self.event()?;
            inline.push(event, &mut spans)?;
        }
        Ok(spans)
    }
}

fn list_gap(in_list: bool) -> u8 {
    if in_list { LIST_GAP } else { BLOCK_GAP }
}

impl Block {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            spans: Vec::new(),
            indent: 0,
            quote: false,
            gap: BLOCK_GAP,
        }
    }
}

impl Span {
    fn plain(text: String) -> Self {
        Self {
            text,
            style: Style::default(),
            image: None,
        }
    }
}

#[derive(Default)]
struct Inline {
    styles: Vec<Style>,
    image: Option<(Image, usize)>,
}

impl Inline {
    fn handling_image(&self) -> bool {
        self.image.is_some()
    }

    fn style(&self) -> Style {
        self.styles.last().cloned().unwrap_or_default()
    }

    fn push(&mut self, event: Event<'_>, spans: &mut Vec<Span>) -> Result<()> {
        if let Some((image, depth)) = &mut self.image {
            match event {
                Event::Start(Tag::Image { .. }) => *depth += 1,
                Event::End(TagEnd::Image) => {
                    *depth -= 1;
                    if *depth == 0 {
                        let (mut image, _) = self.image.take().unwrap();
                        if image.alt.trim().is_empty() {
                            image.alt = "图片".into();
                        }
                        spans.push(Span {
                            image: Some(Box::new(image)),
                            ..Span::plain(String::new())
                        });
                    }
                }
                Event::Text(text) | Event::Code(text) => image.alt.push_str(&text),
                _ => {}
            }
            return Ok(());
        }
        let mut style = self.style();
        let text = match event {
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                self.image = Some((
                    Image {
                        source: dest_url.into_string(),
                        alt: String::new(),
                        title: title.into_string(),
                        link: style.link,
                    },
                    1,
                ));
                return Ok(());
            }
            Event::Start(tag) => {
                match tag {
                    Tag::Strong => style.bold = true,
                    Tag::Emphasis => style.italic = true,
                    Tag::Strikethrough => style.strike = true,
                    Tag::Link { dest_url, .. } => style.link = Some(dest_url.into_string()),
                    _ => return Ok(()),
                }
                ensure!(self.styles.len() < 128, "Markdown 行内格式嵌套超过 128 层");
                self.styles.push(style);
                return Ok(());
            }
            Event::End(
                TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough | TagEnd::Link,
            ) => {
                self.styles.pop();
                return Ok(());
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => text.into_string(),
            Event::Code(text) => {
                style.code = true;
                text.into_string()
            }
            Event::SoftBreak | Event::HardBreak => "\n".into(),
            _ => return Ok(()),
        };
        spans.push(Span {
            text,
            style,
            image: None,
        });
        Ok(())
    }
}

fn is_inline(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::Text(_)
            | Event::Code(_)
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Start(
                Tag::Strong
                    | Tag::Emphasis
                    | Tag::Strikethrough
                    | Tag::Link { .. }
                    | Tag::Image { .. }
            )
            | Event::End(
                TagEnd::Strong
                    | TagEnd::Emphasis
                    | TagEnd::Strikethrough
                    | TagEnd::Link
                    | TagEnd::Image
            )
    )
}
