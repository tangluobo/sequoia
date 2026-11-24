//! Streaming counterpart of the original signature scanner and PE resource extractor.
mod pe;
mod signature;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use signature::{Format, Source};
use std::{
    collections::HashSet,
    fs,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Scan,
    Extract,
    PeOnly,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Options {
    pub source: PathBuf,
    pub output: PathBuf,
    pub mode: Mode,
    pub recursive: bool,
    pub overwrite: bool,
    pub unpack: bool,
    pub pe_scan: bool,
    pub pe_also: bool,
    pub dedup: bool,
    pub verbose: bool,
    pub min_size: u64,
    pub max_file_mib: u64,
    pub categories: Vec<String>,
    pub extensions: Vec<String>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            source: PathBuf::new(),
            output: PathBuf::new(),
            mode: Mode::Scan,
            recursive: false,
            overwrite: false,
            unpack: false,
            pe_scan: true,
            pe_also: false,
            dedup: false,
            verbose: false,
            min_size: 16,
            max_file_mib: 512,
            categories: Vec::new(),
            extensions: Vec::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub source: PathBuf,
    pub offset: u64,
    pub size: u64,
    pub extension: String,
    pub entry: Option<String>,
    pub output: Option<PathBuf>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub hits: Vec<Hit>,
    pub total: usize,
    pub written: usize,
    pub pe_written: usize,
    pub duplicates: usize,
    pub errors: usize,
    pub skipped_files: usize,
    pub log: String,
    pub truncated: bool,
    pub status: String,
}
pub fn formats() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    signature::formats().iter().map(|f| {
        (
            f.extension.as_str(),
            f.description.as_str(),
            f.category.as_str(),
        )
    })
}
struct Run<'a> {
    options: &'a Options,
    report: Report,
    seen: HashSet<(PathBuf, u64)>,
}
fn enabled(options: &Options) -> Vec<&'static Format> {
    let categories: &[String] = &options.categories;
    let extensions: &[String] = &options.extensions;
    signature::formats()
        .iter()
        .filter(|f| {
            (categories.is_empty() || categories.contains(&f.category))
                && (extensions.is_empty() || extensions.contains(&f.extension))
        })
        .collect()
}
fn safe(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
fn stem(value: &str) -> &str {
    value
        .rfind('.')
        .filter(|i| *i > 0)
        .map_or(value, |i| &value[..i])
}
fn source_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
fn output_name(hit: &Hit) -> String {
    let original = hit.entry.as_deref().unwrap_or("");
    let name = hit
        .entry
        .as_deref()
        .map(|s| s.rsplit(['/', '\\']).next().unwrap_or("entry"))
        .map(str::to_owned)
        .unwrap_or_else(|| source_name(&hit.source));
    let mut base = stem(&name).to_owned();
    if base.is_empty() {
        base = "entry".into();
    }
    if !original.is_empty() {
        base.push('_');
        base.push_str(&safe(original));
    }
    format!("{base}_{:06}.{}", hit.offset, hit.extension)
}
fn destination(directory: &Path, name: &str, overwrite: bool) -> Result<PathBuf> {
    let path = directory.join(name);
    if overwrite || !path.try_exists()? {
        return Ok(path);
    }
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..=100000 {
        let path = directory.join(format!("{}_{n}{extension}", stem(name)));
        if !path.try_exists()? {
            return Ok(path);
        }
    }
    anyhow::bail!("输出文件重名过多")
}
fn write_resource(
    reader: &mut impl Read,
    size: u64,
    directory: &Path,
    name: &str,
    overwrite: bool,
) -> Result<PathBuf> {
    fs::create_dir_all(directory)?;
    let directory = fs::canonicalize(directory)?;
    let path = destination(&directory, name, overwrite)?;
    let mut file = tempfile::NamedTempFile::new_in(&directory)?;
    let copied = std::io::copy(&mut reader.take(size), &mut file)?;
    ensure!(copied == size, "资源数据在提取期间被截断");
    file.flush()?;
    file.as_file().sync_all()?;
    if overwrite {
        file.persist(&path).map_err(|e| e.error)?;
    } else {
        file.persist_noclobber(&path).map_err(|e| e.error)?;
    }
    Ok(path)
}
impl Run<'_> {
    fn log(&mut self, text: impl AsRef<str>) {
        let text = text.as_ref();
        if self.report.log.len().saturating_add(text.len() + 1) <= 1024 * 1024 {
            self.report.log.push_str(text);
            self.report.log.push('\n');
        } else {
            self.report.truncated = true;
        }
    }
    fn emit<R: Read + Seek>(
        &mut self,
        path: &Path,
        entry: Option<&str>,
        range: std::ops::Range<u64>,
        extension: &str,
        read_offset: u64,
        source: &mut Source<R>,
    ) -> Result<()> {
        let offset = range.start;
        let size = range.end - range.start;
        self.report.total += 1;
        let mut hit = Hit {
            source: path.to_owned(),
            offset,
            size,
            extension: extension.into(),
            entry: entry.map(str::to_owned),
            output: None,
        };
        if self.options.mode == Mode::Extract {
            ensure!(
                !self.options.dedup || self.seen.len() < 100000,
                "去重记录超过 100000 项"
            );
            if self.options.dedup && !self.seen.insert((path.to_owned(), offset)) {
                self.report.duplicates += 1;
            } else {
                source.reader.seek(SeekFrom::Start(read_offset))?;
                match write_resource(
                    &mut source.reader,
                    size,
                    &self.options.output.join(extension),
                    &output_name(&hit),
                    self.options.overwrite,
                ) {
                    Ok(output) => {
                        self.report.written += 1;
                        if self.options.verbose {
                            self.log(format!("[ok] {}", output.display()));
                        }
                        hit.output = Some(output);
                    }
                    Err(error) => {
                        self.report.errors += 1;
                        self.log(format!("[提取失败] {error:#}"));
                    }
                }
            }
        }
        if self.report.hits.len() < 500 {
            self.report.hits.push(hit);
        } else {
            self.report.truncated = true;
        }
        Ok(())
    }
    fn scan<R: Read + Seek>(
        &mut self,
        path: &Path,
        entry: Option<&str>,
        base: u64,
        source: &mut Source<R>,
        formats: &[&Format],
    ) -> Result<()> {
        signature::scan(
            source,
            formats,
            self.options.min_size,
            |offset, size, format, source| {
                self.emit(
                    path,
                    entry,
                    base + offset..base + offset + size,
                    &format.extension,
                    offset,
                    source,
                )
            },
        )
    }
    fn zip(&mut self, path: &Path, formats: &[&Format]) -> Result<()> {
        let mut file = fs::File::open(path)?;
        // Inspect the bounded end record before the ZIP library builds its central-directory map.
        let length = file.metadata()?.len();
        let tail_length = length.min(65557) as usize;
        file.seek(SeekFrom::End(-(tail_length as i64)))?;
        let mut tail = vec![0; tail_length];
        file.read_exact(&mut tail)?;
        if let Some(at) = tail.windows(4).rposition(|w| w == b"PK\x05\x06")
            && at + 22 <= tail.len()
        {
            let count = u16::from_le_bytes(tail[at + 10..at + 12].try_into()?);
            let bytes = u32::from_le_bytes(tail[at + 12..at + 16].try_into()?);
            ensure!(
                count <= 5000 && bytes <= 8 * 1024 * 1024,
                "ZIP 目录超过 5000 项/8 MiB，未解包"
            );
        }
        file.seek(SeekFrom::Start(0))?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut total = 0u64;
        for index in 0..archive.len().min(5000) {
            let mut entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            if entry.size() > 64 * 1024 * 1024 {
                self.log(format!("[跳过] ZIP 条目超过 64 MiB：{}", entry.name()));
                continue;
            }
            total = total.saturating_add(entry.size());
            ensure!(total <= 512 * 1024 * 1024, "ZIP 解包累计超过 512 MiB");
            let name = entry.name().to_owned();
            let mut data = Vec::new();
            Read::by_ref(&mut entry)
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut data)?;
            ensure!(data.len() <= 64 * 1024 * 1024, "ZIP 条目解包超过 64 MiB");
            let len = data.len() as u64;
            if len == 0 {
                continue;
            }
            let extension = Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let known = signature::formats()
                .iter()
                .find(|f| f.extension == extension);
            let mut source = Source {
                reader: Cursor::new(data),
                len,
            };
            if known.is_some()
                && formats.iter().any(|f| f.extension == extension)
                && !matches!(
                    extension.as_str(),
                    "js" | "json"
                        | "xml"
                        | "html"
                        | "htm"
                        | "css"
                        | "txt"
                        | "cfg"
                        | "ini"
                        | "md"
                        | "py"
                        | "c"
                        | "cpp"
                        | "h"
                        | "java"
                )
            {
                self.emit(path, Some(&name), 0..len, &extension, 0, &mut source)?;
            } else {
                self.scan(path, Some(&name), 0, &mut source, formats)?;
            }
        }
        if archive.len() > 5000 {
            self.log("ZIP 条目超过 5000 项，其余条目未解包");
            self.report.truncated = true;
        }
        Ok(())
    }
    fn file(&mut self, path: &Path, formats: &[&Format]) -> Result<()> {
        let file = fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len == 0 {
            return Ok(());
        }
        if len > self.options.max_file_mib * 1024 * 1024 {
            self.report.skipped_files += 1;
            self.log(format!("[跳过] 文件超过大小限制：{}", path.display()));
            return Ok(());
        }
        let mut source = Source { reader: file, len };
        if self.options.mode != Mode::PeOnly {
            self.scan(path, None, 0, &mut source, formats)?;
            if self.options.unpack
                && source
                    .slice(0, 4.min(len as usize))?
                    .starts_with(b"PK\x03\x04")
                && let Err(error) = self.zip(path, formats)
            {
                self.report.errors += 1;
                self.log(format!("[ZIP] {error:#}"));
            }
        }
        let is_pe = source.len >= 64 && source.slice(0, 2)? == b"MZ";
        if !is_pe
            || !(self.options.pe_scan || self.options.pe_also || self.options.mode == Mode::PeOnly)
        {
            return Ok(());
        }
        let entries = pe::parse(&mut source)?;
        if self.options.pe_scan && self.options.mode != Mode::PeOnly {
            for entry in &entries {
                if entry.size < self.options.min_size {
                    continue;
                }
                let resource_name = format!(
                    "{}_{}",
                    entry
                        .kind_name
                        .as_deref()
                        .map(str::to_owned)
                        .unwrap_or_else(|| format!("RT{}", entry.kind)),
                    entry.name
                );
                let normalized = entry
                    .kind_name
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .to_ascii_lowercase();
                let normalized = match normalized.as_str() {
                    "jpeg" | "jpgimage" => "jpg",
                    "bitmap" => "bmp",
                    "icon" => "ico",
                    "cursor" => "cur",
                    name => name,
                };
                if let Some(format) = formats.iter().find(|f| f.extension == normalized) {
                    self.emit(
                        path,
                        Some(&resource_name),
                        entry.offset..entry.offset + entry.size,
                        &format.extension,
                        entry.offset,
                        &mut source,
                    )?;
                } else {
                    // Only one PE resource is materialized for nested scanning at a time.
                    ensure!(
                        entry.size <= 64 * 1024 * 1024,
                        "单个 PE 扫描资源超过 64 MiB"
                    );
                    let data = source.slice(entry.offset, entry.size as usize)?;
                    let mut section = Source {
                        reader: Cursor::new(data),
                        len: entry.size,
                    };
                    self.scan(
                        path,
                        Some(&resource_name),
                        entry.offset,
                        &mut section,
                        formats,
                    )?;
                }
            }
        }
        if (self.options.pe_also || self.options.mode == Mode::PeOnly) && pe_extension(path) {
            let output = if self.options.output.as_os_str().is_empty() {
                std::env::current_dir()?.join("extracted_pe")
            } else {
                self.options.output.join("pe")
            };
            pe::extract(&mut source, &entries, |name, extension, data| {
                let filename = format!("{}_{}.{}", stem(&source_name(path)), name, extension);
                match write_resource(
                    &mut data.as_slice(),
                    data.len() as u64,
                    &output.join(extension),
                    &filename,
                    self.options.overwrite,
                ) {
                    Ok(file) => {
                        self.report.pe_written += 1;
                        if self.options.verbose {
                            self.log(format!("[PE] {}", file.display()));
                        }
                    }
                    Err(error) => {
                        self.report.errors += 1;
                        self.log(format!("[PE 提取失败] {error:#}"));
                    }
                }
                Ok(())
            })?;
        }
        Ok(())
    }
}
fn files(source: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if source.is_file() {
        return Ok(vec![source.to_owned()]);
    }
    ensure!(source.is_dir(), "源路径不存在：{}", source.display());
    let mut pending = vec![source.to_owned()];
    let mut files = Vec::new();
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_file() {
                files.push(entry.path());
            } else if kind.is_dir() && recursive {
                pending.push(entry.path());
            }
            ensure!(
                files.len() + pending.len() <= 100000,
                "扫描目录超过 100000 项"
            );
        }
    }
    files.sort();
    Ok(files)
}
fn companions(path: &Path) -> Vec<PathBuf> {
    if !pe_extension(path) {
        return Vec::new();
    }
    let mut result = Vec::new();
    let Some(parent) = path.parent() else {
        return result;
    };
    let name = source_name(path);
    result.push(parent.join(format!("{name}.mun")));
    if let Some(windows) = std::env::var_os("WINDIR") {
        result.push(
            PathBuf::from(windows)
                .join("SystemResources")
                .join(format!("{name}.mun")),
        );
    }
    if let Ok(dirs) = fs::read_dir(parent) {
        for entry in dirs.take(10000).flatten() {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                result.push(entry.path().join(format!("{name}.mui")));
            }
        }
    }
    result.retain(|p| p.is_file());
    result
}
fn pe_extension(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        matches!(
            s.to_ascii_lowercase().as_str(),
            "exe" | "dll" | "scr" | "fon" | "sys" | "ocx" | "mun" | "mui"
        )
    })
}
pub fn execute(options: Options) -> Result<Report> {
    ensure!(
        (16..=1024).contains(&options.min_size) && (64..=2048).contains(&options.max_file_mib),
        "资源大小选项超出范围"
    );
    ensure!(
        options.mode == Mode::Scan || !options.output.as_os_str().is_empty(),
        "请选择输出目录"
    );
    let mut targets = files(&options.source, options.recursive)?;
    let original = targets.len();
    let mut unique: HashSet<_> = targets.iter().cloned().collect();
    for index in 0..original {
        for path in companions(&targets[index]) {
            if unique.insert(path.clone()) {
                targets.push(path);
            }
        }
        ensure!(targets.len() <= 110000, "伴随资源过多");
    }
    let formats = enabled(&options);
    let mut run = Run {
        options: &options,
        report: Report::default(),
        seen: HashSet::new(),
    };
    run.log(format!(
        "源: {}\n模式: {:?}\n递归: {}\n最小大小: {} bytes\n最大文件: {} MiB",
        options.source.display(),
        options.mode,
        options.recursive,
        options.min_size,
        options.max_file_mib
    ));
    for path in targets {
        if options.mode == Mode::PeOnly && !pe_extension(&path) {
            continue;
        }
        if options.verbose {
            run.log(format!("[scan] {}", path.display()));
        }
        if let Err(error) = run.file(&path, &formats) {
            run.report.errors += 1;
            run.log(format!("[错误] {}：{error:#}", path.display()));
        }
    }
    run.report.status = match options.mode {
        Mode::Scan => format!("扫描完成！共发现 {} 个资源", run.report.total),
        Mode::Extract => format!(
            "提取完成！共 {} 个资源，成功提取 {} 个",
            run.report.total, run.report.written
        ),
        Mode::PeOnly => format!("PE资源提取完成: {} 个文件", run.report.pe_written),
    };
    if run.report.errors > 0 {
        run.report
            .status
            .push_str(&format!("（{} 项失败，详见日志）", run.report.errors));
    }
    Ok(run.report)
}
