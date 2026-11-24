use super::*;
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::Value;
use std::fmt;
fn value(v: Value) -> Result<Cell> {
    Ok(match v {
        Value::Null => Cell::Null,
        Value::Bool(b) => Cell::Boolean(b),
        Value::Number(n) => Cell::Text(n.to_string()),
        Value::String(s) => Cell::Text(s),
        v => Cell::Text(serde_json::to_string(&v)?),
    })
}
fn emit(
    sink: &mut Sink,
    record: Value,
    source: &Source,
    number: &mut u64,
    control: &Control,
) -> Result<()> {
    *number += 1;
    if !included(source, *number) {
        return Ok(());
    }
    let pairs = match record {
        Value::Object(m) => m.into_iter().collect::<Vec<_>>(),
        Value::Array(v) => v
            .into_iter()
            .enumerate()
            .map(|(i, v)| (format!("f{}", i + 1), v))
            .collect(),
        v => vec![("value".into(), v)],
    };
    let mut row = vec![Cell::Null; sink.table.fields.len()];
    for (name, v) in pairs {
        let index = sink.field(&name)?;
        row.resize(sink.table.fields.len(), Cell::Null);
        row[index] = value(v)?;
    }
    sink.push(row, control)
}
struct Rows<'a> {
    sink: &'a mut Sink,
    source: &'a Source,
    path: &'a [String],
    number: &'a mut u64,
    control: &'a Control,
    found: &'a mut bool,
    budget: std::rc::Rc<std::cell::Cell<usize>>,
}
impl<'de> DeserializeSeed<'de> for Rows<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> std::result::Result<(), D::Error> {
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for Rows<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "JSON 记录数组或对象")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> std::result::Result<(), A::Error> {
        if let Some(part) = self.path.first() {
            let target = part.parse::<usize>().map_err(de::Error::custom)?;
            let mut i = 0;
            while i < target && seq.next_element::<de::IgnoredAny>()?.is_some() {
                i += 1
            }
            if i == target {
                seq.next_element_seed(Rows {
                    path: &self.path[1..],
                    ..self
                })?;
            }
            while seq.next_element::<de::IgnoredAny>()?.is_some() {}
            return Ok(());
        }
        *self.found = true;
        loop {
            self.budget.set(0);
            let Some(record) = seq.next_element::<Value>()? else {
                break;
            };
            emit(self.sink, record, self.source, self.number, self.control)
                .map_err(de::Error::custom)?;
        }
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
        if let Some(part) = self.path.first() {
            while let Some(key) = map.next_key::<String>()? {
                if &key == part {
                    map.next_value_seed(Rows {
                        sink: &mut *self.sink,
                        source: self.source,
                        path: &self.path[1..],
                        number: &mut *self.number,
                        control: self.control,
                        found: &mut *self.found,
                        budget: self.budget.clone(),
                    })?
                } else {
                    map.next_value::<de::IgnoredAny>()?;
                }
            }
            return Ok(());
        }
        let mut object = serde_json::Map::new();
        while let Some(key) = map.next_key::<String>()? {
            if object.contains_key(&key) {
                return Err(de::Error::custom("JSON 记录包含重复字段"));
            }
            object.insert(key, map.next_value::<Value>()?);
        }
        *self.found = true;
        emit(
            self.sink,
            Value::Object(object),
            self.source,
            self.number,
            self.control,
        )
        .map_err(de::Error::custom)
    }
}
pub(super) fn json(
    source: &Source,
    path: &Path,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    let mut sink = Sink::new(
        directory,
        output,
        name(path),
        path.display().to_string(),
        Vec::new(),
    )?;
    let mut number = 0;
    let (input, budget) = Budget::new(super::delimited::Decoded::open(source, path)?);
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jsonl") || e.eq_ignore_ascii_case("ndjson"))
    {
        let mut records =
            serde_json::Deserializer::from_reader(BufReader::new(input)).into_iter::<Value>();
        loop {
            budget.set(0);
            let Some(record) = records.next() else { break };
            control.check()?;
            emit(&mut sink, record?, source, &mut number, control)?;
        }
    } else {
        let p = source.json_path.trim();
        let p = p.strip_prefix("$.").unwrap_or(p);
        let parts = if p.is_empty() || p == "$" {
            Vec::new()
        } else if let Some(stripped) = p.strip_prefix('/') {
            stripped
                .split('/')
                .map(|p| p.replace("~1", "/").replace("~0", "~"))
                .collect()
        } else {
            p.split('.').map(str::to_owned).collect()
        };
        ensure!(parts.len() <= 64, "JSON 路径超过 64 层");
        let mut decoder = serde_json::Deserializer::from_reader(BufReader::new(input));
        let mut found = false;
        Rows {
            sink: &mut sink,
            source,
            path: &parts,
            number: &mut number,
            control,
            found: &mut found,
            budget,
        }
        .deserialize(&mut decoder)?;
        decoder.end()?;
        ensure!(found, "JSON 记录路径不存在");
    }
    sink.finish(output)
}
struct XmlField {
    name: String,
    null: bool,
    encoding: String,
    text: String,
    raw: quick_xml::Writer<Vec<u8>>,
    nested: bool,
    depth: usize,
}
fn unhex(s: &str) -> Result<Vec<u8>> {
    ensure!(
        s.len().is_multiple_of(2),
        "XML 二进制字段的十六进制长度无效"
    );
    s.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| Ok(u8::from_str_radix(std::str::from_utf8(p)?, 16)?))
        .collect()
}
pub(super) fn xml(
    source: &Source,
    path: &Path,
    directory: &Path,
    output: &mut Catalog,
    control: &Control,
) -> Result<()> {
    use base64::Engine as _;
    use quick_xml::events::Event;
    let (input, budget) = Budget::new(super::delimited::Decoded::open(source, path)?);
    let mut reader = quick_xml::Reader::from_reader(BufReader::new(input));
    reader.config_mut().expand_empty_elements = true;
    let mut sink = Sink::new(
        directory,
        output,
        name(path),
        path.display().to_string(),
        Vec::new(),
    )?;
    let mut buffer = Vec::new();
    let mut stack = Vec::<String>::new();
    let mut row_depth = 0;
    let mut number = 0;
    let mut row = Vec::<Cell>::new();
    let mut column: Option<XmlField> = None;
    let mut seen = std::collections::HashSet::new();
    loop {
        control.check()?;
        let event = reader.read_event_into(&mut buffer)?;
        match &event {
            Event::Start(e) => {
                let tag = String::from_utf8(e.local_name().as_ref().to_vec())?;
                stack.push(tag.clone());
                ensure!(stack.len() <= 128, "XML 嵌套超过 128 层");
                let record = if source.xml_row.is_empty() {
                    tag.eq_ignore_ascii_case("row")
                        || tag.eq_ignore_ascii_case("record")
                        || stack.len() == 2
                } else {
                    stack.join("/").ends_with(&source.xml_row) || tag == source.xml_row
                };
                if row_depth == 0 && record {
                    row_depth = stack.len();
                    row = vec![Cell::Null; sink.table.fields.len()];
                    seen.clear();
                    for attribute in e.attributes() {
                        let a = attribute?;
                        let key = String::from_utf8(a.key.as_ref().to_vec())?;
                        if key.starts_with("xmlns") {
                            continue;
                        }
                        let index = sink.field(&format!("@{key}"))?;
                        row.resize(sink.table.fields.len(), Cell::Null);
                        row[index] =
                            Cell::Text(a.decode_and_unescape_value(reader.decoder())?.into_owned());
                    }
                } else if row_depth > 0 && stack.len() == row_depth + 1 {
                    let mut field = XmlField {
                        name: tag,
                        null: false,
                        encoding: String::new(),
                        text: String::new(),
                        raw: quick_xml::Writer::new(Vec::new()),
                        nested: false,
                        depth: stack.len(),
                    };
                    for attribute in e.attributes() {
                        let a = attribute?;
                        let v = a.decode_and_unescape_value(reader.decoder())?;
                        match a.key.as_ref() {
                            b"name" => field.name = v.into_owned(),
                            b"null" | b"xsi:nil" => field.null = v == "true" || v == "1",
                            b"encoding" => field.encoding = v.into_owned(),
                            _ => {}
                        }
                    }
                    column = Some(field);
                } else if let Some(field) = &mut column {
                    field.nested = true;
                    field.raw.write_event(event.clone().into_owned())?;
                }
            }
            Event::End(_) => {
                if column.as_ref().is_some_and(|f| f.depth == stack.len()) {
                    let field = column.take().unwrap();
                    ensure!(
                        seen.insert(field.name.clone()),
                        "XML 记录包含重复字段 {}",
                        field.name
                    );
                    let index = sink.field(&field.name)?;
                    row.resize(sink.table.fields.len(), Cell::Null);
                    let text = if field.nested {
                        String::from_utf8(field.raw.into_inner())?
                    } else {
                        field.text
                    };
                    row[index] = if field.null {
                        Cell::Null
                    } else if field.encoding.eq_ignore_ascii_case("hex") {
                        Cell::Binary(unhex(&text)?)
                    } else if field.encoding.eq_ignore_ascii_case("base64") {
                        Cell::Binary(base64::engine::general_purpose::STANDARD.decode(text.trim())?)
                    } else {
                        Cell::Text(text)
                    };
                } else if let Some(field) = &mut column {
                    field.raw.write_event(event.clone().into_owned())?;
                }
                if row_depth == stack.len() {
                    number += 1;
                    if included(source, number) {
                        sink.push(std::mem::take(&mut row), control)?
                    }
                    row_depth = 0;
                    budget.set(0);
                }
                stack.pop();
            }
            Event::Text(e) => {
                if let Some(field) = &mut column {
                    field
                        .text
                        .push_str(&quick_xml::escape::unescape(&e.decode()?)?);
                    field.raw.write_event(event.clone().into_owned())?;
                }
            }
            Event::CData(e) => {
                if let Some(field) = &mut column {
                    field.text.push_str(&e.decode()?);
                    field.raw.write_event(event.clone().into_owned())?;
                }
            }
            Event::GeneralRef(e) => {
                if let Some(field) = &mut column {
                    let escaped = format!("&{};", e.decode()?);
                    field.text.push_str(&quick_xml::escape::unescape(&escaped)?);
                    field.raw.write_event(event.clone().into_owned())?;
                }
            }
            Event::DocType(_) => anyhow::bail!("XML 导入不支持 DTD，请使用不含外部实体的数据文件"),
            Event::Eof => break,
            _ => {}
        }
        if let Some(field) = &column {
            ensure!(
                field.text.len() + field.raw.get_ref().len() <= MAX_ROW_BYTES,
                "XML 字段超过 16 MiB"
            );
        }
        buffer.clear();
    }
    ensure!(row_depth == 0 && stack.is_empty(), "XML 记录不完整");
    sink.finish(output)
}
