//! VOC input is bounded per document; COCO images/annotations are streamed.
use super::{Counts, directory::Files};
use crate::backup::Control;
use anyhow::{Context as _, Result, ensure};
use serde::Serialize;
use serde_json::json;
use std::{
    collections::BTreeMap,
    io::{BufWriter, Read as _, Seek as _, Write as _},
    path::Path,
};

fn read_xml(path: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 8 * 1024 * 1024, "VOC XML 文件超过8 MiB");
    let (encoding, skip) = if let Some(value) = encoding_rs::Encoding::for_bom(&bytes) {
        value
    } else if bytes.starts_with(&[0, b'<', 0, b'?']) {
        (encoding_rs::UTF_16BE, 0)
    } else if bytes.starts_with(&[b'<', 0, b'?', 0]) {
        (encoding_rs::UTF_16LE, 0)
    } else {
        let mut reader = quick_xml::Reader::from_reader(bytes.as_slice());
        let encoding = if let quick_xml::events::Event::Decl(decl) = reader.read_event()? {
            if let Some(label) = decl.encoding() {
                encoding_rs::Encoding::for_label(&label?).context("XML 编码无效")?
            } else {
                encoding_rs::UTF_8
            }
        } else {
            encoding_rs::UTF_8
        };
        (encoding, 0)
    };
    let (text, errors) = encoding.decode_without_bom_handling(&bytes[skip..]);
    ensure!(!errors, "XML 文本编码无效");
    Ok(text.into_owned())
}
fn field(parent: roxmltree::Node<'_, '_>, tag: &str) -> Option<String> {
    let node = parent
        .descendants()
        .find(|n| n.is_element() && n.tag_name().name() == tag)?;
    let text: String = node
        .descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect();
    Some(text.trim_matches(|c: char| c <= ' ').to_owned())
}
fn integer(parent: roxmltree::Node<'_, '_>, tag: &str) -> Option<i32> {
    field(parent, tag)?.parse().ok()
}
fn append(
    writer: &mut impl std::io::Write,
    first: &mut bool,
    value: &impl Serialize,
) -> Result<()> {
    if !*first {
        writer.write_all(b",")?;
    }
    serde_json::to_writer(writer, value)?;
    *first = false;
    Ok(())
}
#[derive(Serialize)]
struct Category {
    id: u64,
    name: String,
    supercategory: &'static str,
}

pub(in crate::tools) fn convert(
    source: &Path,
    destination: &Path,
    control: &Control,
) -> Result<Counts> {
    control.check()?;
    ensure!(source.is_dir(), "源数据集目录不存在");
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let mut output = tempfile::Builder::new()
        .prefix(".tomato-coco-")
        .tempfile_in(parent)?;
    let mut annotation_file = tempfile::tempfile_in(parent)?;
    let mut writer = BufWriter::new(output.as_file_mut());
    let mut annotation_writer = BufWriter::new(&mut annotation_file);
    let now = || {
        chrono::Local::now()
            .naive_local()
            .format("%Y-%m-%dT%H:%M:%S%.f")
            .to_string()
    };
    writer.write_all(b"{\"info\":")?;
    serde_json::to_writer(
        &mut writer,
        &json!({"description":"Converted from PASCAL VOC","version":"1.0","year":2026,"contributor":"Tomato","date_created":now()}),
    )?;
    writer.write_all(b",\"licenses\":[{\"id\":1,\"name\":\"Unknown\",\"url\":\"http://unknown.org\"}],\"images\":[")?;
    let mut files = Files::new(source, "xml")?;
    let mut counts = Counts::default();
    let (mut first_image, mut first_annotation) = (true, true);
    let mut categories = Vec::new();
    let mut category_ids = BTreeMap::new();
    let mut category_bytes = 0usize;
    while let Some(path) = files.next(control)? {
        counts.total += 1;
        let Ok(xml) = read_xml(&path) else {
            counts.failed += 1;
            continue;
        };
        let Ok(document) = roxmltree::Document::parse_with_options(
            &xml,
            roxmltree::ParsingOptions {
                allow_dtd: false,
                nodes_limit: 262_144,
            },
        ) else {
            counts.failed += 1;
            continue;
        };
        let root = document.root_element();
        let (Some(filename), Some(width), Some(height)) = (
            field(root, "filename"),
            integer(root, "width"),
            integer(root, "height"),
        ) else {
            counts.failed += 1;
            continue;
        };
        let image_id = counts.success + 1;
        append(
            &mut writer,
            &mut first_image,
            &json!({"id":image_id,"file_name":filename,"width":width,"height":height,"license":1,"flickr_url":"","coco_url":"","date_captured":now()}),
        )?;
        counts.success += 1;
        for object in root
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "object")
        {
            control.check()?;
            let Some(name) = field(object, "name") else {
                continue;
            };
            let Some(bounds) = object
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "bndbox")
            else {
                continue;
            };
            let (Some(xmin), Some(ymin), Some(xmax), Some(ymax)) = (
                integer(bounds, "xmin"),
                integer(bounds, "ymin"),
                integer(bounds, "xmax"),
                integer(bounds, "ymax"),
            ) else {
                continue;
            };
            let category_id = if let Some(id) = category_ids.get(&name) {
                *id
            } else {
                ensure!(
                    categories.len() < 65_536
                        && category_bytes.saturating_add(name.len()) <= 4 * 1024 * 1024,
                    "数据集类别超过65536项或4 MiB名称预算"
                );
                let id = categories.len() as u64 + 1;
                category_bytes += name.len();
                category_ids.insert(name.clone(), id);
                categories.push(Category {
                    id,
                    name,
                    supercategory: "object",
                });
                id
            };
            // Java uses signed Integer coordinates and does not clamp boxes to
            // the image. Preserve its values, including explicitly clipped or
            // negative training annotations, instead of rejecting the dataset.
            let width = xmax.wrapping_sub(xmin);
            let height = ymax.wrapping_sub(ymin);
            counts.records += 1;
            append(
                &mut annotation_writer,
                &mut first_annotation,
                &json!({"id":counts.records,"image_id":image_id,"category_id":category_id,"bbox":[xmin,ymin,width,height],"area":width.wrapping_mul(height),"segmentation":[],"iscrowd":0}),
            )?;
        }
    }
    ensure!(counts.total > 0, "源数据集目录中没有找到 XML 文件");
    ensure!(counts.success > 0, "没有成功解析任何 XML 文件");
    control.check()?;
    annotation_writer.flush()?;
    drop(annotation_writer);
    annotation_file.rewind()?;
    writer.write_all(b"],\"annotations\":[")?;
    let mut buffer = [0; 64 * 1024];
    loop {
        control.check()?;
        let count = annotation_file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        writer.write_all(&buffer[..count])?;
    }
    writer.write_all(b"],\"categories\":")?;
    serde_json::to_writer(&mut writer, &categories)?;
    writer.write_all(b"}")?;
    writer.flush()?;
    drop(writer);
    control.check()?;
    output.as_file().sync_all()?;
    output.persist(destination).map_err(|e| e.error)?;
    Ok(counts)
}
