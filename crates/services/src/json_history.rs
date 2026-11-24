//! Legacy JSON history repository. Stream records; retain only their small summaries.
use anyhow::{Context, Result, ensure};
use ring::digest;
use serde::{
    Deserialize, Serialize,
    de::{Error as _, SeqAccess, Visitor},
    ser::{SerializeSeq, Serializer as _},
};
use std::{
    collections::BTreeMap,
    fmt, fs,
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
const MAX_RECORDS: usize = tomato_core::limits::JSON_HISTORY_ENTRIES;
const MAX_DOCUMENT: usize = tomato_core::limits::JSON_HISTORY_BYTES;
const MAX_FILE: u64 = 512 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub index: usize,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub key: Key,
    pub timestamp: String,
    pub summary: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub enum Action {
    Load,
    Format { json: String },
    Restore(Key),
    Copy(Key),
    Remove(Key),
    Clear,
}
impl fmt::Debug for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load => f.write_str("Load"),
            Self::Clear => f.write_str("Clear"),
            Self::Format { json } => f
                .debug_struct("Format")
                .field("bytes", &json.len())
                .finish(),
            Self::Restore(key) => f.debug_tuple("Restore").field(key).finish(),
            Self::Copy(key) => f.debug_tuple("Copy").field(key).finish(),
            Self::Remove(key) => f.debug_tuple("Remove").field(key).finish(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Output {
    pub items: Vec<Item>,
    pub input: Option<String>,
    pub formatted: Option<String>,
    pub clipboard: Option<String>,
    pub notice: Option<String>,
}
#[derive(Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    json: String,
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    summary: String,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}
pub fn legacy_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(".tomata/json_history.json")
}
fn hash(text: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(digest::digest(&digest::SHA256, text.as_bytes()))
}
fn summary(text: &str) -> String {
    let mut result = String::new();
    let mut whitespace = false;
    let mut units = 0;
    for c in text.trim().chars() {
        if c.is_whitespace() {
            whitespace = true;
            continue;
        }
        if whitespace && !result.is_empty() {
            if units == 40 {
                result.push_str("...");
                break;
            }
            result.push(' ');
            units += 1;
        }
        whitespace = false;
        if units + c.len_utf16() > 40 {
            result.push_str("...");
            break;
        }
        result.push(c);
        units += c.len_utf16();
    }
    result
}
struct HashReader<R> {
    reader: R,
    hash: digest::Context,
}
impl<R: Read> Read for HashReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.reader.read(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }
}
fn scan(
    path: &Path,
    mut visit: impl FnMut(usize, Record) -> Result<()>,
) -> Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let length = file.metadata()?.len();
    ensure!(length <= MAX_FILE, "JSON历史文件超过512 MiB");
    let mut reader = HashReader {
        reader: BufReader::new(file).take(MAX_FILE + 1),
        hash: digest::Context::new(&digest::SHA256),
    };
    struct Records<'a, F>(&'a mut F);
    impl<'de, F: FnMut(usize, Record) -> Result<()>> Visitor<'de> for Records<'_, F> {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("原版JSON历史记录数组")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
            let mut index = 0;
            while let Some(record) = seq.next_element::<Record>()? {
                if index >= 10_000 || record.json.len() > MAX_DOCUMENT {
                    return Err(A::Error::custom("JSON历史记录超过容量限制，原文件保持不变"));
                }
                (self.0)(index, record).map_err(A::Error::custom)?;
                index += 1;
            }
            Ok(())
        }
    }
    if length != 0 {
        use serde::de::Deserializer;
        let mut de = serde_json::Deserializer::from_reader(&mut reader);
        de.deserialize_seq(Records(&mut visit))?;
        de.end()?;
    }
    Ok(Some(reader.hash.finish().as_ref().to_vec()))
}
fn load(path: &Path) -> Result<Vec<Item>> {
    let mut items = Vec::new();
    scan(path, |index, record| {
        if index < MAX_RECORDS && !record.json.is_empty() {
            items.push(Item {
                key: Key {
                    index,
                    digest: hash(&record.json),
                },
                timestamp: record.timestamp,
                summary: if record.summary.is_empty() {
                    summary(&record.json)
                } else {
                    record.summary
                },
            });
        }
        Ok(())
    })?;
    Ok(items)
}
pub fn execute(path: &Path, action: Action) -> Result<Output> {
    match execute_inner(path, &action) {
        Ok(output) => Ok(output),
        Err(error) => {
            if let Action::Format { json } = action {
                Ok(Output {
                    items: load(path).unwrap_or_default(),
                    input: None,
                    formatted: Some(tomato_core::tools::json_transform(&json, "format")?),
                    clipboard: None,
                    notice: Some(format!("格式化成功，历史记录未保存：{error:#}")),
                })
            } else {
                Err(error)
            }
        }
    }
}
fn execute_inner(path: &Path, action: &Action) -> Result<Output> {
    let mut formatted = None;
    let mut input = None;
    let mut clipboard = None;
    if matches!(action, Action::Load) {
        return Ok(Output {
            items: load(path)?,
            input,
            formatted,
            clipboard,
            notice: None,
        });
    }
    let mut found = false;
    let revision = scan(path, |index, record| {
        match &action {
            Action::Restore(key) | Action::Remove(key) | Action::Copy(key)
                if index == key.index =>
            {
                ensure!(
                    hash(&record.json) == key.digest,
                    "历史记录已被外部修改，请刷新后重试"
                );
                found = true;
                if matches!(action, Action::Restore(_)) {
                    input = Some(record.json);
                } else if matches!(action, Action::Copy(_)) {
                    clipboard = Some(record.json);
                }
            }
            _ => {}
        }
        Ok(())
    })?;
    if matches!(
        action,
        Action::Restore(_) | Action::Remove(_) | Action::Copy(_)
    ) {
        ensure!(found, "历史记录已变化，请刷新后重试");
    }
    if let Some(json) = &input {
        formatted = Some(tomato_core::tools::json_transform(json, "format")?);
    }
    let new_record = if let Action::Format { json } = &action {
        ensure!(json.len() <= MAX_DOCUMENT, "JSON输入超过8 MiB");
        let text = tomato_core::tools::json_transform(json, "format")?;
        ensure!(
            text.len() <= MAX_DOCUMENT,
            "格式化后历史记录超过8 MiB，未保存到历史文件"
        );
        // The Java implementation stores formatted text and checks exact duplicates.
        let mut duplicate = false;
        scan(path, |_, r| {
            duplicate |= r.json == text;
            Ok(())
        })?;
        let record = Record {
            summary: summary(&text),
            json: text.clone(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            extra: Default::default(),
        };
        formatted = Some(text);
        (!duplicate).then_some(record)
    } else {
        None
    };
    let mutate = matches!(action, Action::Clear | Action::Remove(_)) || new_record.is_some();
    if mutate {
        let parent = path.parent().context("历史文件缺少父目录")?;
        fs::create_dir_all(parent)?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            let mut serializer = serde_json::Serializer::pretty(&mut writer);
            let mut sequence = serializer.serialize_seq(None)?;
            let mut count = 0;
            if let Some(record) = &new_record {
                sequence.serialize_element(record)?;
                count += 1;
            }
            if !matches!(action, Action::Clear) {
                scan(path, |index, record| {
                    if let Action::Remove(key) = &action
                        && index == key.index
                    {
                        return Ok(());
                    }
                    if count < MAX_RECORDS {
                        sequence.serialize_element(&record)?;
                        count += 1;
                    }
                    Ok(())
                })?;
            }
            sequence.end()?;
            writer.flush()?;
        }
        ensure!(
            scan(path, |_, _| Ok(()))? == revision,
            "历史文件已被外部修改，保存已取消"
        );
        if revision.is_some() {
            let backup = path.with_extension("json.bak");
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(backup)
            {
                Ok(mut backup) => {
                    std::io::copy(&mut fs::File::open(path)?, &mut backup)?;
                    backup.sync_all()?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| e.error)?;
    }
    Ok(Output {
        items: load(path)?,
        input,
        formatted,
        clipboard,
        notice: None,
    })
}
