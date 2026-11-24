use anyhow::Result;
use std::{
    collections::BTreeMap,
    fs,
    io::{Cursor, Write},
};
use tomato_services::extractor::{self, Mode, Options};
fn png() -> Result<Vec<u8>> {
    let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        16,
        16,
        image::Rgba([10, 180, 80, 255]),
    ));
    let mut data = Cursor::new(Vec::new());
    image.write_to(&mut data, image::ImageFormat::Png)?;
    Ok(data.into_inner())
}
#[test]
fn signatures_stream_across_windows_and_extraction_preserves_existing_outputs() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("嵌入文件.bin");
    let output = root.path().join("out");
    let png = png()?;
    let mut bytes = vec![0x61; 65530];
    bytes.extend_from_slice(&png);
    let pdf = bytes.len();
    bytes.extend_from_slice(b"%PDF-1.7\nexample document\n%%EOF");
    fs::write(&source, &bytes)?;
    let options = Options {
        source,
        output: output.clone(),
        mode: Mode::Extract,
        pe_scan: false,
        ..Default::default()
    };
    let report = extractor::execute(options.clone())?;
    assert_eq!(report.errors, 0);
    assert_eq!(report.total, 2);
    assert_eq!(report.written, 2);
    assert_eq!(report.hits[0].offset, 65530);
    assert_eq!(report.hits[0].size, png.len() as u64);
    assert_eq!(report.hits[1].offset, pdf as u64);
    assert_eq!(fs::read(report.hits[0].output.as_ref().unwrap())?, png);
    let report = extractor::execute(options)?;
    assert!(
        report.hits[0]
            .output
            .as_ref()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("_1.png")
    );
    let formats: Vec<_> = extractor::formats().collect();
    assert_eq!(formats.len(), 58);
    assert_eq!(
        formats
            .iter()
            .map(|f| f.0)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        48
    );
    Ok(())
}
#[test]
fn results_keep_all_counts_with_bounded_previews_and_skip_oversized_files() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("many.bin");
    let image = png()?;
    let data = image.repeat(550);
    fs::write(&source, &data)?;
    let report = extractor::execute(Options {
        source: source.clone(),
        pe_scan: false,
        ..Default::default()
    })?;
    assert_eq!(report.total, 550);
    assert_eq!(report.hits.len(), 500);
    assert!(report.truncated);
    assert_eq!(fs::read(&source)?, data);
    fs::File::create(&source)?.set_len(65 * 1024 * 1024)?;
    let report = extractor::execute(Options {
        source,
        max_file_mib: 64,
        ..Default::default()
    })?;
    assert_eq!(report.skipped_files, 1);
    assert_eq!(report.total, 0);
    Ok(())
}
#[test]
fn zip_resources_keep_legacy_source_offset_dedup_and_cannot_escape_output() -> Result<()> {
    let root = tempfile::tempdir()?;
    let source = root.path().join("resources.zip");
    let data = png()?;
    let mut zip = zip::ZipWriter::new(fs::File::create(&source)?);
    for name in ["../../logo.png", "second.png"] {
        zip.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        zip.write_all(&data)?;
    }
    zip.finish()?;
    let options = Options {
        source,
        output: root.path().join("out"),
        mode: Mode::Extract,
        unpack: true,
        pe_scan: false,
        ..Default::default()
    };
    let report = extractor::execute(options.clone())?;
    assert_eq!(report.errors, 0);
    assert_eq!(report.written, 2);
    let output = fs::canonicalize(&options.output)?;
    for hit in &report.hits {
        assert!(hit.output.as_ref().unwrap().starts_with(&output));
        assert_eq!(fs::read(hit.output.as_ref().unwrap())?, data);
    }
    let report = extractor::execute(Options {
        dedup: true,
        output: root.path().join("dedup"),
        ..options
    })?;
    assert_eq!(report.written, 1);
    assert_eq!(report.duplicates, 1);
    assert_eq!(report.total, 2);
    Ok(())
}
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    Named(String),
    Id(u32),
}
enum Node {
    Directory(Vec<(Key, Node)>),
    Data(Vec<u8>),
}
fn put16(data: &mut [u8], at: usize, n: u16) {
    data[at..at + 2].copy_from_slice(&n.to_le_bytes());
}
fn put32(data: &mut [u8], at: usize, n: u32) {
    data[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn tree(node: &Node, data: &mut Vec<u8>, payloads: &mut Vec<(usize, Vec<u8>)>) -> usize {
    let at = data.len();
    match node {
        Node::Data(bytes) => {
            data.resize(at + 16, 0);
            put32(data, at + 4, bytes.len() as u32);
            payloads.push((at, bytes.clone()));
        }
        Node::Directory(children) => {
            data.resize(at + 16 + children.len() * 8, 0);
            put16(
                data,
                at + 12,
                children
                    .iter()
                    .filter(|(key, _)| matches!(key, Key::Named(_)))
                    .count() as u16,
            );
            put16(
                data,
                at + 14,
                children
                    .iter()
                    .filter(|(key, _)| matches!(key, Key::Id(_)))
                    .count() as u16,
            );
            for (index, (key, child)) in children.iter().enumerate() {
                let raw = match key {
                    Key::Named(name) => {
                        let start = data.len();
                        let chars: Vec<_> = name.encode_utf16().collect();
                        data.extend_from_slice(&(chars.len() as u16).to_le_bytes());
                        for c in chars {
                            data.extend_from_slice(&c.to_le_bytes());
                        }
                        0x8000_0000 | start as u32
                    }
                    Key::Id(id) => *id,
                };
                let offset = tree(child, data, payloads) as u32
                    | if matches!(child, Node::Directory(_)) {
                        0x8000_0000
                    } else {
                        0
                    };
                put32(data, at + 16 + index * 8, raw);
                put32(data, at + 20 + index * 8, offset);
            }
        }
    }
    at
}
fn pe_fixture() -> Result<Vec<u8>> {
    let image = png()?;
    let mut group = vec![0; 34];
    put16(&mut group, 2, 1);
    put16(&mut group, 4, 2);
    group[6] = 16;
    group[7] = 16;
    put16(&mut group, 10, 1);
    put16(&mut group, 12, 32);
    put32(&mut group, 14, image.len() as u32);
    put16(&mut group, 18, 1);
    put16(&mut group, 32, 999);
    let mut cursor = vec![0; 20];
    put16(&mut cursor, 2, 2);
    put16(&mut cursor, 4, 1);
    put16(&mut cursor, 6, 16);
    put16(&mut cursor, 8, 16);
    put16(&mut cursor, 18, 5);
    let mut cursor_image = vec![2, 0, 3, 0];
    cursor_image.extend_from_slice(&image);
    let mut bitmap = vec![0; 44];
    put32(&mut bitmap, 0, 40);
    put32(&mut bitmap, 4, 1);
    put32(&mut bitmap, 8, 1);
    put16(&mut bitmap, 12, 1);
    put16(&mut bitmap, 14, 24);
    let mut kinds = BTreeMap::<Key, Vec<(Key, Node)>>::new();
    for (kind, id, data) in [
        (Key::Id(3), Key::Id(1), image.clone()),
        (Key::Id(14), Key::Id(10), group),
        (Key::Id(1), Key::Id(5), cursor_image),
        (Key::Id(12), Key::Id(11), cursor),
        (Key::Id(2), Key::Id(2), bitmap),
        (Key::Id(24), Key::Id(1), b"<assembly/>".to_vec()),
        (Key::Named("PNG".into()), Key::Named("LOGO".into()), image),
    ] {
        kinds
            .entry(kind)
            .or_default()
            .push((id, Node::Directory(vec![(Key::Id(1033), Node::Data(data))])));
    }
    let root = Node::Directory(
        kinds
            .into_iter()
            .map(|(key, children)| (key, Node::Directory(children)))
            .collect(),
    );
    let mut resources = Vec::new();
    let mut payloads = Vec::new();
    tree(&root, &mut resources, &mut payloads);
    for (at, payload) in payloads {
        while resources.len() % 4 != 0 {
            resources.push(0);
        }
        let offset = resources.len();
        put32(&mut resources, at, 0x1000 + offset as u32);
        resources.extend_from_slice(&payload);
    }
    let mut bytes = vec![0; 0x200];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, 0x84, 0x8664);
    put16(&mut bytes, 0x86, 1);
    put16(&mut bytes, 0x94, 240);
    put16(&mut bytes, 0x98, 0x20b);
    put32(&mut bytes, 0x98 + 108, 16);
    put32(&mut bytes, 0x98 + 128, 0x1000);
    put32(&mut bytes, 0x98 + 132, resources.len() as u32);
    let section = 0x98 + 240;
    bytes[section..section + 5].copy_from_slice(b".rsrc");
    put32(&mut bytes, section + 8, resources.len() as u32);
    put32(&mut bytes, section + 12, 0x1000);
    put32(&mut bytes, section + 16, resources.len() as u32);
    put32(&mut bytes, section + 20, 0x200);
    bytes.extend_from_slice(&resources);
    Ok(bytes)
}
#[test]
fn pe_native_groups_cursor_hotspots_bitmaps_manifests_named_sections_and_companions() -> Result<()>
{
    let root = tempfile::tempdir()?;
    let source = root.path().join("sample.exe");
    let bytes = pe_fixture()?;
    fs::write(&source, &bytes)?;
    let options = Options {
        source: source.clone(),
        output: root.path().join("out"),
        mode: Mode::PeOnly,
        ..Default::default()
    };
    let report = extractor::execute(options.clone())?;
    assert_eq!(report.errors, 0, "{}", report.log);
    assert_eq!(report.pe_written, 6);
    let output = options.output.join("pe");
    assert_eq!(
        fs::read(output.join("png/sample_icon_1_lang1033.png"))?,
        png()?
    );
    let ico = fs::read(output.join("ico/sample_icon_10.ico"))?;
    assert_eq!(&ico[..6], [0, 0, 1, 0, 1, 0]);
    assert_eq!(&ico[22..], png()?.as_slice());
    let cursor = fs::read(output.join("cur/sample_cursor_11.cur"))?;
    assert_eq!(&cursor[10..14], [2, 0, 3, 0]);
    assert_eq!(&cursor[22..], png()?.as_slice());
    let bitmap = fs::read(output.join("bmp/sample_bitmap_2.bmp"))?;
    assert_eq!(&bitmap[..2], b"BM");
    assert_eq!(u32::from_le_bytes(bitmap[10..14].try_into()?), 54);
    assert_eq!(
        fs::read(output.join("xml/sample_manifest.xml"))?,
        b"<assembly/>"
    );
    let scan = extractor::execute(Options {
        source: source.clone(),
        ..Default::default()
    })?;
    assert_eq!(scan.errors, 0, "{}", scan.log);
    assert!(
        scan.hits
            .iter()
            .any(|h| h.entry.as_deref() == Some("PNG_LOGO") && h.extension == "png")
    );
    fs::write(root.path().join("sample.exe.mun"), bytes)?;
    let report = extractor::execute(Options {
        output: root.path().join("companions"),
        ..options
    })?;
    assert_eq!(report.pe_written, 12);
    assert_eq!(report.errors, 0, "{}", report.log);
    Ok(())
}
#[test]
fn damaged_pe_reports_errors_without_panics_or_allocating_declared_resource_size() -> Result<()> {
    let root = tempfile::tempdir()?;
    for index in 0..32 {
        let mut bytes = pe_fixture()?;
        let offset = if index % 2 == 0 { 0x3c } else { 0x98 + 128 };
        put32(&mut bytes, offset, u32::MAX - index);
        fs::write(root.path().join(format!("damaged{index}.exe")), bytes)?;
    }
    let report = extractor::execute(Options {
        source: root.path().into(),
        output: root.path().join("out"),
        mode: Mode::PeOnly,
        ..Default::default()
    })?;
    assert_eq!(report.pe_written, 0);
    assert!(report.errors > 0);
    Ok(())
}

#[test]
#[ignore = "Set TOMATO_ORIGINAL_JAVA_ROOT and TOMATO_ORACLE_JDK; compiles read-only original extractor classes into a disposable folder"]
fn same_files_match_original_java_scan_offsets_output_names_and_converted_bytes() -> Result<()> {
    use base64::Engine;
    let root = tempfile::tempdir()?;
    let original = std::path::PathBuf::from(
        std::env::var_os("TOMATO_ORIGINAL_JAVA_ROOT").expect("original Java source root required"),
    );
    let jdk = std::path::PathBuf::from(
        std::env::var_os("TOMATO_ORACLE_JDK").expect("oracle JDK required"),
    );
    let classes = root.path().join("classes");
    fs::create_dir(&classes)?;
    let sources = original.join("src/main/java/com/tangluobo/tomato/module/tools/extractor");
    let executable = |name: &str| {
        jdk.join("bin")
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    };
    let mut compile = std::process::Command::new(executable("javac"));
    compile.args(["-encoding", "UTF-8", "-d"]).arg(&classes);
    for folder in ["core", "format", "pe", "utils"] {
        for entry in fs::read_dir(sources.join(folder))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "java") {
                compile.arg(path);
            }
        }
    }
    compile.arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/java/ExtractorOracle.java"),
    );
    let output = compile.output()?;
    anyhow::ensure!(
        output.status.success(),
        "original extractor compilation failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let png = png()?;
    let binary = root.path().join("sample.exe");
    fs::write(&binary, pe_fixture()?)?;
    let embedded = root.path().join("embedded.bin");
    let mut data = vec![0x61; 65530];
    data.extend_from_slice(&png);
    data.extend_from_slice(b"%PDF-1.7\nfixture document\n%%EOF");
    fs::write(&embedded, data)?;
    let archive = root.path().join("assets.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&archive)?);
    zip.start_file(
        "assets/logo.png",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )?;
    zip.write_all(&png)?;
    zip.finish()?;
    for (index, source) in [&binary, &embedded, &archive].into_iter().enumerate() {
        for mode in ["scan", "extract", "pe"] {
            if mode == "pe" && index != 0 {
                continue;
            }
            let java_output = root.path().join(format!("java-{index}-{mode}"));
            let rust_output = root.path().join(format!("rust-{index}-{mode}"));
            let output = std::process::Command::new(executable("java"))
                .arg("-cp")
                .arg(&classes)
                .arg("ExtractorOracle")
                .arg(source)
                .arg(mode)
                .arg(&java_output)
                .output()?;
            anyhow::ensure!(
                output.status.success(),
                "original oracle failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let lines = String::from_utf8(output.stdout)?;
            let report = extractor::execute(Options {
                source: source.to_owned(),
                output: rust_output.clone(),
                mode: match mode {
                    "extract" => Mode::Extract,
                    "pe" => Mode::PeOnly,
                    _ => Mode::Scan,
                },
                unpack: true,
                ..Default::default()
            })?;
            assert_eq!(report.errors, 0, "{}", report.log);
            let mut actual: Vec<_> = report
                .hits
                .iter()
                .map(|h| {
                    format!(
                        "H\t{}\t{}\t{}\t{}",
                        h.offset,
                        h.size,
                        h.extension,
                        base64::engine::general_purpose::STANDARD
                            .encode(h.entry.as_deref().unwrap_or(""))
                    )
                })
                .collect();
            actual.sort();
            let mut expected: Vec<_> = lines
                .lines()
                .filter(|l| l.starts_with("H\t"))
                .map(str::to_owned)
                .collect();
            expected.sort();
            assert_eq!(
                actual,
                expected,
                "scan mismatch for {mode}: {}",
                source.display()
            );
            let mut actual = Vec::new();
            let mut pending = vec![rust_output.clone()];
            while let Some(dir) = pending.pop() {
                if !dir.is_dir() {
                    continue;
                }
                for entry in fs::read_dir(dir)? {
                    let path = entry?.path();
                    if path.is_dir() {
                        pending.push(path);
                    } else {
                        let bytes = fs::read(&path)?;
                        let sha = ring::digest::digest(&ring::digest::SHA256, &bytes);
                        let sha: String = sha.as_ref().iter().map(|b| format!("{b:02x}")).collect();
                        actual.push(format!(
                            "F\t{}\t{sha}",
                            path.strip_prefix(&rust_output)?
                                .to_string_lossy()
                                .replace('\\', "/")
                        ));
                    }
                }
            }
            actual.sort();
            let mut expected: Vec<_> = lines
                .lines()
                .filter(|l| l.starts_with("F\t"))
                .map(str::to_owned)
                .collect();
            expected.sort();
            assert_eq!(
                actual,
                expected,
                "output mismatch for {mode}: {}",
                source.display()
            );
        }
    }
    Ok(())
}
