//! Chinese column-title clipboard format shared with the original Java designer.
use super::*;
use anyhow::{Result, ensure};
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
    ser::{SerializeMap, SerializeSeq},
};
use std::{
    fmt,
    io::{self, Write},
};
const LIMIT: usize = 8 * 1024 * 1024;
const TITLES: [&str; 15] = [
    "字段名",
    "类型",
    "长度",
    "小数点",
    "非空",
    "主键",
    "自增",
    "默认值",
    "注释",
    "无符号",
    "填充零",
    "字符集",
    "排序规则",
    "键长度",
    "二进制",
];

struct Scalar(String);
impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = Scalar;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a scalar field value")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Scalar, E> {
                Ok(Scalar(value.into()))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Scalar, E> {
                Ok(Scalar(value))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Scalar, E> {
                Ok(Scalar(String::new()))
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Scalar, E> {
                Ok(Scalar(value.to_string()))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Scalar, E> {
                Ok(Scalar(value.to_string()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Scalar, E> {
                Ok(Scalar(value.to_string()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Scalar, E> {
                Ok(Scalar(value.to_string()))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Scalar, A::Error> {
                serde_json::Number::deserialize(de::value::MapAccessDeserializer::new(map))
                    .map(|value| Scalar(value.to_string()))
            }
        }
        deserializer.deserialize_any(Read)
    }
}
struct Row([String; 15]);
impl<'de> Deserialize<'de> for Row {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = Row;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a field object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Row, A::Error> {
                let mut row = std::array::from_fn(|_| String::new());
                while let Some(key) = map.next_key::<String>()? {
                    if let Some(index) = TITLES.iter().position(|title| *title == key) {
                        row[index] = map.next_value::<Scalar>()?.0;
                    } else {
                        map.next_value::<de::IgnoredAny>()?;
                    }
                }
                Ok(Row(row))
            }
        }
        deserializer.deserialize_map(Read)
    }
}
struct Rows(Vec<Row>);
impl<'de> Deserialize<'de> for Rows {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Read;
        impl<'de> Visitor<'de> for Read {
            type Value = Rows;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list of fields")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Rows, A::Error> {
                let mut rows = Vec::new();
                while let Some(row) = seq.next_element()? {
                    if rows.len() == 4096 {
                        return Err(de::Error::custom("字段超过 4,096 项"));
                    }
                    rows.push(row);
                }
                Ok(Rows(rows))
            }
        }
        deserializer.deserialize_seq(Read)
    }
}
struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > LIMIT.saturating_sub(self.0.len()) {
            return Err(io::Error::other("字段剪贴板超过 8 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Gson;
impl serde_json::ser::Formatter for Gson {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (index, ch) in fragment.char_indices() {
            let escaped = match ch {
                '<' => "\\u003c",
                '>' => "\\u003e",
                '&' => "\\u0026",
                '=' => "\\u003d",
                '\'' => "\\u0027",
                '\u{2028}' => "\\u2028",
                '\u{2029}' => "\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..index])?;
            writer.write_all(escaped.as_bytes())?;
            start = index + ch.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}
struct BorrowedRow<'a>(&'a Draft, usize);
impl Serialize for BorrowedRow<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(15))?;
        let c = &self.0.request.columns[self.1];
        let flag = |value: bool, bit: usize| self.0.flags[self.1].text(value, bit);
        let values = [
            c.name.as_str(),
            &c.data_type,
            &c.length,
            &c.scale,
            flag(c.not_null, 0),
            flag(c.primary_key, 1),
            flag(c.auto_increment, 2),
            &c.default_value,
            &c.comment,
            flag(c.unsigned, 3),
            flag(c.zerofill, 4),
            &c.charset,
            &c.collation,
            &c.key_length,
            flag(c.binary, 5),
        ];
        for (title, value) in TITLES.into_iter().zip(values) {
            map.serialize_entry(title, value)?;
        }
        map.end()
    }
}
struct BorrowedRows<'a>(&'a Draft, &'a [usize]);
impl Serialize for BorrowedRows<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.1.len()))?;
        for &row in self.1 {
            seq.serialize_element(&BorrowedRow(self.0, row))?;
        }
        seq.end()
    }
}
impl Draft {
    pub fn copy_fields(&mut self) -> Result<Option<&str>> {
        let mut rows = self.selected_rows();
        if rows.is_empty()
            && let Some(cell) = self
                .focus
                .filter(|cell| cell.row < self.request.columns.len())
        {
            rows.push(cell.row);
        }
        if rows.is_empty() {
            self.status = "请先选择要复制的字段".into();
            return Ok(None);
        }
        let mut writer = Bounded(Vec::new());
        BorrowedRows(self, &rows).serialize(&mut serde_json::Serializer::with_formatter(
            &mut writer,
            Gson,
        ))?;
        self.copied_fields = Some(String::from_utf8(writer.0)?);
        self.status = format!("已复制 {} 个字段到剪贴板", rows.len());
        Ok(self.copied_fields.as_deref())
    }
    pub fn paste_fields(&mut self, ctx: &egui::Context, clipboard: &str) -> Result<()> {
        if self.busy {
            return Ok(());
        }
        self.commit_edit(ctx);
        let text = self
            .copied_fields
            .as_deref()
            .filter(|s| !s.trim_matches(|ch: char| ch <= ' ').is_empty())
            .unwrap_or(clipboard);
        ensure!(text.len() <= LIMIT, "字段剪贴板超过 8 MiB");
        if text.trim_matches(|ch: char| ch <= ' ').is_empty() {
            self.status = "剪贴板无内容".into();
            return Ok(());
        }
        let rows = serde_json::from_str::<Option<Rows>>(text)?
            .map(|rows| rows.0)
            .unwrap_or_default();
        if rows.is_empty() {
            self.status = "剪贴板中无可粘贴的字段".into();
            return Ok(());
        }
        ensure!(
            self.request.columns.len() + rows.len() <= 4096,
            "字段超过 4,096 项"
        );
        let start = self.focus.map_or(self.request.columns.len(), |cell| {
            cell.row.saturating_add(1).min(self.request.columns.len())
        });
        let count = rows.len();
        let types = self.types();
        let mut columns = Vec::with_capacity(count);
        let mut flags = Vec::with_capacity(count);
        for Row(mut row) in rows {
            let flag = |index: usize| row[index] == "是";
            let mut column = Column {
                not_null: flag(4),
                primary_key: flag(5),
                auto_increment: flag(6),
                unsigned: flag(9),
                zerofill: flag(10),
                binary: flag(14),
                ..Default::default()
            };
            let original_flags = Flags::from_strings(
                [4, 5, 6, 9, 10, 14].map(|index| std::mem::take(&mut row[index])),
            );
            let [
                name,
                data_type,
                length,
                scale,
                _,
                _,
                _,
                default_value,
                comment,
                _,
                _,
                charset,
                collation,
                key_length,
                _,
            ] = row;
            column.name = name;
            column.data_type = data_type;
            column.length = length;
            column.scale = scale;
            column.default_value = default_value;
            column.default_expression = false;
            column.comment = comment;
            column.charset = charset;
            column.collation = collation;
            column.key_length = key_length;
            column.infer_if_unset(self.dialect, types);
            columns.push(column);
            flags.push(original_flags);
        }
        self.request.columns.splice(start..start, columns);
        self.flags.splice(start..start, flags);
        self.origins
            .splice(start..start, std::iter::repeat_n(None, count));
        self.clear_selection();
        self.selected.resize(self.request.columns.len(), 0);
        self.selected[start..start + count].fill(0xff);
        self.focus = Some(Cell {
            row: start + count - 1,
            column: 7,
        });
        self.changed();
        self.status = format!("已粘贴 {count} 个字段（未保存）");
        Ok(())
    }
}
