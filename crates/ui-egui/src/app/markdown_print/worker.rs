use super::{gpu, layout};
use anyhow::{Context as _, Result, bail, ensure};
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tomato_services::{
    backup::Control,
    commands::Output,
    files::{document, object_tree},
    markdown::{self, Picture},
};

pub(super) struct Request {
    pub name: String,
    pub source: String,
    pub document: PathBuf,
    pub remote: Option<Arc<tomato_core::Connection>>,
    pub client: Option<document::Client>,
    pub fonts: egui::FontDefinitions,
    pub owner: tomato_native_print::Owner,
    pub graphics: gpu::Graphics,
    pub fixture: Option<PathBuf>,
}

pub(in crate::app) struct Report {
    name: String,
    pages: usize,
    missing: usize,
}
impl Report {
    pub fn message(&self) -> String {
        let mut message = format!("已发送到打印机：{}（{} 页）", self.name, self.pages);
        if self.missing > 0 {
            message.push_str(&format!(
                "\n{} 张图片无法加载，页面保留了错误说明。",
                self.missing
            ));
        }
        message
    }
}

pub(super) fn validate_source(source: &str) -> Result<()> {
    ensure!(!source.trim().is_empty(), "没有可打印的内容");
    // The print graph is retained for pagination. Keep its glyph and shape count bounded.
    ensure!(source.len() <= 768 * 1024, "Markdown 打印最多支持 768 KiB");
    Ok(())
}

pub(super) async fn run(mut request: Request, control: &Control) -> Result<Option<Report>> {
    let source = std::mem::take(&mut request.source);
    let parsed = tokio::task::spawn_blocking(move || {
        validate_source(&source)?;
        markdown::parse(&source)
    })
    .await??;
    let images = tokio::select! {
        _ = control.wait_cancelled() => return Ok(None),
        result = load_images(&request, &parsed, control) => result?,
    };
    let control = control.clone();
    tokio::task::spawn_blocking(move || spool(request, parsed, images, &control)).await?
}

async fn load_images(
    request: &Request,
    parsed: &markdown::Document,
    control: &Control,
) -> Result<HashMap<String, Result<Picture>>> {
    let mut images = HashMap::new();
    let mut bytes = 0usize;
    for image in parsed.images() {
        let source = &image.source;
        control.check()?;
        if images.contains_key(source) {
            continue;
        }
        ensure!(images.len() < 256, "Markdown 打印最多支持 256 张不同的图片");
        let mut picture = picture(request, source).await;
        if let Ok(image) = &mut picture {
            image.gif = None;
            let next = bytes.saturating_add(image.rgba.len());
            if next <= 32 * 1024 * 1024 {
                bytes = next;
            } else {
                picture = Err(anyhow::anyhow!("打印图片超过 32 MiB 总预算"));
            }
        }
        images.insert(source.clone(), picture);
    }
    Ok(images)
}

async fn picture(request: &Request, source: &str) -> Result<Picture> {
    let output = if let Some(client) = &request.client {
        client
            .execute(document::Action::Image {
                document: request.document.to_string_lossy().replace('\\', "/"),
                source: source.into(),
            })
            .await?
    } else if let Some(connection) = &request.remote {
        object_tree::execute(
            connection,
            object_tree::Request::Image {
                document: request.document.clone(),
                source: source.into(),
            },
        )
        .await?
    } else {
        return markdown::picture(request.document.clone(), source.into()).await;
    };
    match output {
        Output::MarkdownImage(picture) => Ok(picture),
        _ => bail!("图片服务器返回了错误的响应"),
    }
}

fn spool(
    request: Request,
    parsed: markdown::Document,
    images: HashMap<String, Result<Picture>>,
    control: &Control,
) -> Result<Option<Report>> {
    control.check()?;
    let missing = images.values().filter(|image| image.is_err()).count();
    let limit = request.graphics.device.limits().max_texture_dimension_2d as usize;
    let mut layout = layout::Layout::new(parsed, request.fonts, images, limit.min(8192))?;
    control.check()?;
    let mut printer = if let Some(path) = &request.fixture {
        ensure!(!path.exists(), "验收 PDF 路径已存在");
        tomato_native_print::Printer::pdf(layout.len())?
    } else {
        let Some(printer) = tomato_native_print::Printer::select(request.owner, layout.len())?
        else {
            return Ok(None);
        };
        printer
    };
    control.check()?;
    let mut renderer = gpu::Pages::new(request.graphics, &mut layout)?;
    let [width, height] = renderer.size();
    let range = printer.pages();
    let pages = range.len();
    control.start(pages as u64);
    printer.begin(&request.name, request.fixture.as_deref())?;
    for page in range {
        control.check()?;
        let pixels = renderer
            .render(&layout, page)
            .with_context(|| format!("渲染第 {} 页失败", page + 1))?;
        control.check()?;
        printer.page(width, height, pixels)?;
        control.object();
    }
    control.check()?;
    printer.finish()?;
    Ok(Some(Report {
        name: request.name,
        pages,
        missing,
    }))
}
