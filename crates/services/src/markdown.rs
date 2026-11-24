//! CommonMark parsing is a background use case, independent of egui.
use anyhow::Result;
use serde::Serialize;
mod export;
mod preview;
pub(crate) mod resource;
mod tables;
pub use export::{export_html, html_filename};
pub use preview::parse;
pub const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
#[derive(Clone, Debug, Default, Serialize)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Span {
    pub text: String,
    pub style: Style,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<Box<Image>>,
}
#[derive(Clone, Debug, Serialize)]
pub enum Kind {
    Paragraph,
    Heading(u8),
    Code(String),
    HtmlBlock,
    Rule,
    Table(Vec<Vec<Vec<Span>>>),
}
#[derive(Clone, Debug, Serialize)]
pub struct Image {
    pub source: String,
    pub alt: String,
    pub title: String,
    pub link: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Block {
    pub kind: Kind,
    pub spans: Vec<Span>,
    pub indent: usize,
    pub quote: bool,
    pub gap: u8,
}
#[derive(Debug, Default, Serialize)]
pub struct Document {
    pub blocks: Vec<Block>,
}
impl Document {
    pub fn images(&self) -> impl Iterator<Item = &Image> {
        self.blocks
            .iter()
            .flat_map(|block| {
                let cells = match &block.kind {
                    Kind::Table(rows) => rows.as_slice(),
                    _ => &[],
                };
                block.spans.iter().chain(cells.iter().flatten().flatten())
            })
            .filter_map(|span| span.image.as_deref())
    }
}
pub fn html(source: &str) -> Result<String> {
    let mut output = Vec::new();
    export::render(source, "导出", &mut output)?;
    Ok(String::from_utf8(output)?)
}
#[derive(Debug, Serialize)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    #[serde(skip)]
    pub gif: Option<crate::files::images::DocumentGif>,
}
pub async fn picture(document: std::path::PathBuf, source: String) -> Result<Picture> {
    resource::picture(&document, &source).await
}
/// Matches MarkdownEditorPane.resolveRemoteResourcePath for object storage keys.
pub fn remote_resource_key(document: &str, source: &str) -> String {
    let source = resource::relative_path(source).replace('\\', "/");
    let base = document.replace('\\', "/");
    let mut parts: Vec<_> = if source.starts_with('/') {
        Vec::new()
    } else {
        base.rsplit_once('/')
            .map(|(p, _)| p.split('/').filter(|s| !s.is_empty()).collect())
            .unwrap_or_default()
    };
    for part in source.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_resources_match_original_percent_plus_query_and_bucket_root_rules() {
        for (source, expected) in [
            ("../a%20b+%E4%B8%AD.png?size=1#x", "docs/a b+中.png"),
            ("/root.png", "root.png"),
            ("../../../../root.png", "root.png"),
            ("./x\\y.png", "docs/sub/x/y.png"),
            ("a%ZZ.png", "docs/sub/a%ZZ.png"),
        ] {
            assert_eq!(remote_resource_key("docs/sub/file.md", source), expected);
        }
    }
    #[test]
    fn commonmark_preview_preserves_nested_styles_lists_tables_and_code() -> Result<()> {
        let doc = parse(
            "# 标题\n\n**加粗 *斜体*** [链接](https://example.org)\n\n- 列表\n- [x] 完成\n\n> 引用\n\n```rust\nlet x = 1;\n```\n\n| A | B |\n|---|---|\n| 甲 | 乙 |\n\n![图](images/test.png)",
        )?;
        assert!(matches!(doc.blocks[0].kind, Kind::Heading(1)));
        assert!(
            doc.blocks
                .iter()
                .flat_map(|block| &block.spans)
                .any(|span| span.style.bold && span.style.italic && span.text == "斜体")
        );
        assert!(doc.blocks.iter().any(|block| block.quote));
        assert!(
            doc.blocks
                .iter()
                .any(|block| matches!(&block.kind,Kind::Code(language) if language=="rust"))
        );
        assert!(doc.blocks.iter().any(|block|matches!(&block.kind,Kind::Table(rows) if rows.len()==2&&rows[1][0][0].text=="甲")));
        assert!(
            doc.images()
                .any(|image| image.source == "images/test.png" && image.alt == "图")
        );
        assert!(doc.blocks.iter().any(|block| {
            block.spans.first().is_some_and(|span| span.text == "• ") && block.spans.len() > 1
        }));
        let exported = html("# 标题\n\n`<&>`")?;
        assert!(exported.contains("<h1>标题</h1>"));
        assert!(exported.contains("&lt;&amp;&gt;"));
        let source = "第一行\n第二行 <span>原样</span>\n\n<div>块级 HTML</div>\n\n末尾";
        let doc = parse(source)?;
        assert_eq!(doc.blocks.len(), 3);
        assert_eq!(
            doc.blocks[0]
                .spans
                .iter()
                .map(|s| s.text.as_str())
                .collect::<String>(),
            "第一行\n第二行 <span>原样</span>"
        );
        assert!(matches!(doc.blocks[1].kind, Kind::HtmlBlock));
        assert!(doc.blocks[1].spans[0].text.contains("<div>块级 HTML</div>"));
        Ok(())
    }
    #[tokio::test]
    async fn local_preview_image_is_decoded_off_thread_and_limits_are_enforced() -> Result<()> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("a b+图#%.png");
        image::RgbaImage::from_pixel(4, 3, image::Rgba([20, 30, 40, 255])).save(&path)?;
        let file_url = url::Url::from_file_path(&path).unwrap();
        let data_url = format!(
            "DaTa:image/png;BASE64,{}",
            STANDARD.encode(std::fs::read(&path)?)
        );
        let document = dir.path().join("README.md");
        let mut sources = vec![
            "a%20b+%E5%9B%BE%23%25.png?ignored#ignored".to_owned(),
            file_url.to_string(),
            data_url,
        ];
        if cfg!(windows) {
            sources.push(path.to_string_lossy().into_owned());
        }
        for source in sources {
            let picture = picture(document.clone(), source).await?;
            assert_eq!((picture.width, picture.height), (4, 3));
            assert_eq!(picture.rgba, [20, 30, 40, 255].repeat(12));
        }
        for source in [
            "",
            "data:invalid",
            "data:image/png;base64,%%%",
            "file://unsupported-host/a.png",
            "ftp://example.invalid/a.png",
            "%00.png",
        ] {
            assert!(
                picture(document.clone(), source.into()).await.is_err(),
                "{source}"
            );
        }
        let oversized = dir.path().join("large.png");
        std::fs::File::create(&oversized)?.set_len((crate::files::images::MAX_BYTES + 1) as u64)?;
        assert!(
            picture(document, "large.png".into())
                .await
                .unwrap_err()
                .to_string()
                .contains("8 MiB")
        );
        Ok(())
    }

    #[test]
    fn preview_images_preserve_java_alt_title_and_wrapping_link() -> Result<()> {
        let doc = parse(
            "[![粗体 **中文** `code` ![内层](nested.png)](外层.png \"图标题\")](https://example.invalid/)\n\n![]()",
        )?;
        let images: Vec<_> = doc.images().collect();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].source, "外层.png");
        assert_eq!(images[0].alt, "粗体 中文 code 内层");
        assert_eq!(images[0].title, "图标题");
        assert_eq!(images[0].link.as_deref(), Some("https://example.invalid/"));
        assert_eq!(images[1].alt, "图片");
        assert!(images[1].source.is_empty());
        Ok(())
    }

    #[test]
    fn preview_keeps_text_and_images_in_their_original_heading_paragraph_and_cell() -> Result<()> {
        let doc = parse(
            "# left ![heading](a.png) right\n\nA ![body](b.png) B\n\n| X | Y |\n|---|---|\n| C ![cell](c.png) D | ![second](d.png) |",
        )?;
        assert_eq!(doc.blocks.len(), 3);
        assert!(matches!(doc.blocks[0].kind, Kind::Heading(1)));
        assert_eq!(doc.blocks[0].spans[0].text, "left ");
        assert_eq!(doc.blocks[0].spans[2].text, " right");
        assert_eq!(doc.blocks[1].spans[2].text, " B");
        let Kind::Table(rows) = &doc.blocks[2].kind else {
            anyhow::bail!("missing table");
        };
        assert_eq!(rows[1][0][2].text, " D");
        assert_eq!(
            doc.images()
                .map(|image| image.source.as_str())
                .collect::<Vec<_>>(),
            ["a.png", "b.png", "c.png", "d.png"]
        );
        Ok(())
    }

    #[test]
    fn preview_applies_java_table_grammar_and_cell_paragraph_rules() -> Result<()> {
        let doc = parse(
            "LEAD\nX | YY | Z\n:--- | ---:\na\\|b | **strong** | tail | DISCARD\n| - literal | `code\\|span` |\n| lone |\n\nAFTER",
        )?;
        assert_eq!(doc.blocks.len(), 3);
        let Kind::Table(rows) = &doc.blocks[1].kind else {
            anyhow::bail!("missing Java table");
        };
        assert_eq!(rows.len(), 4);
        assert!(rows.iter().all(|row| row.len() == 3));
        assert_eq!(rows[1][0][0].text, "a|b");
        assert!(rows[1][1][0].style.bold);
        assert_eq!(rows[2][0][0].text, "- literal");
        assert_eq!(rows[2][1][0].text, "code|span");
        assert!(rows[2][1][0].style.code && rows[2][2].is_empty() && rows[3][1].is_empty());
        Ok(())
    }

    #[test]
    fn nested_lists_keep_java_inset_numbering_paragraphs_and_group_gaps() -> Result<()> {
        let doc = parse(
            "7. Top\n   - Child\n\n     3. Ordered child\n     4. Second ordered\n   - Next\n8. Next top\n\n   Continuation\n\nAfter",
        )?;
        let actual: Vec<_> = doc
            .blocks
            .iter()
            .map(|block| {
                let text = block
                    .spans
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<String>();
                (text, block.indent, block.gap)
            })
            .collect();
        let expected = [
            ("7. Top", 1, 2),
            ("• Child", 1, 2),
            ("3. Ordered child", 1, 2),
            ("4. Second ordered", 1, 2),
            ("• Next", 1, 2),
            ("8. Next top", 1, 2),
            ("Continuation", 1, 6),
            ("After", 0, 6),
        ];
        assert_eq!(
            actual,
            expected.map(|(text, indent, gap)| (text.to_string(), indent, gap))
        );
        Ok(())
    }
}
