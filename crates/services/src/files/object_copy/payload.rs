use std::io::{Error, ErrorKind, Result};
pub const MAX_ITEMS: usize = 1000;
pub const MAX_TEXT: usize = 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub key: String,
    pub directory: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub connection_id: String,
    pub kind: String,
    pub bucket: String,
    pub items: Vec<Item>,
}
fn invalid() -> Error {
    Error::new(
        ErrorKind::InvalidData,
        "对象复制数据无效，或超过1000项/1 MiB",
    )
}
fn field(value: &str) -> bool {
    !value.is_empty() && !value.contains(['\n', '\r', '\0'])
}
impl Selection {
    pub fn validate(&self) -> Result<()> {
        if !field(&self.connection_id)
            || !matches!(self.kind.as_str(), "S3" | "ALIYUN_OSS" | "LOCAL_DIRECTORY")
            || !field(&self.bucket)
            || self.bucket.contains(['/', '\\'])
            || self.items.is_empty()
            || self.items.len() > MAX_ITEMS
        {
            return Err(invalid());
        }
        let mut retained = self
            .connection_id
            .len()
            .saturating_add(self.kind.len())
            .saturating_add(self.bucket.len())
            .saturating_add(3);
        for item in &self.items {
            if !field(&item.key) || item.key.trim_matches('/').is_empty() {
                return Err(invalid());
            }
            retained = retained.saturating_add(item.key.len()).saturating_add(3);
        }
        if retained > MAX_TEXT {
            return Err(invalid());
        }
        Ok(())
    }
    pub fn parse(text: &str) -> Result<Self> {
        if text.len() > MAX_TEXT {
            return Err(invalid());
        }
        let mut lines = text.split('\n');
        let connection_id = lines.next().ok_or_else(invalid)?.to_owned();
        let kind = lines.next().ok_or_else(invalid)?.to_owned();
        let bucket = lines.next().ok_or_else(invalid)?.to_owned();
        let mut items = Vec::new();
        for line in lines.filter(|line| !line.is_empty()) {
            let Some((tag, key)) = line.split_once('|') else {
                return Err(invalid());
            };
            if items.len() == MAX_ITEMS || !matches!(tag, "D" | "F") {
                return Err(invalid());
            }
            items.push(Item {
                key: key.into(),
                directory: tag == "D",
            });
        }
        let selection = Self {
            connection_id,
            kind,
            bucket,
            items,
        };
        selection.validate()?;
        Ok(selection)
    }
    pub fn text(&self) -> Result<String> {
        self.validate()?;
        let mut text = format!("{}\n{}\n{}\n", self.connection_id, self.kind, self.bucket);
        for item in &self.items {
            text.push(if item.directory { 'D' } else { 'F' });
            text.push('|');
            text.push_str(&item.key);
            text.push('\n');
        }
        Ok(text)
    }
    pub fn description(&self) -> String {
        let directories = self.items.iter().filter(|i| i.directory).count();
        let files = self.items.len() - directories;
        if directories == 0 {
            format!("{files} 个文件")
        } else {
            format!("{files} 个文件, {directories} 个目录")
        }
    }
}
impl Item {
    pub fn basename(&self) -> &str {
        self.key
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("")
    }
    pub fn destination(&self, prefix: &str) -> String {
        let base = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", prefix.trim_end_matches('/'))
        };
        format!(
            "{base}{}{}",
            self.basename(),
            if self.directory { "/" } else { "" }
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_text_keeps_nested_unicode_and_pipe_names_and_destination_basename() -> Result<()> {
        let text = "fixture-id\nS3\nsource-bucket\nD|目录/🍅/\nF|dir/a|b.txt\n";
        let selection = Selection::parse(text)?;
        assert_eq!(selection.text()?, text);
        assert_eq!(selection.description(), "1 个文件, 1 个目录");
        assert_eq!(selection.items[0].destination("new/path/"), "new/path/🍅/");
        assert_eq!(selection.items[1].destination(""), "a|b.txt");
        for text in [
            "id\nFTP\nbucket\nF|x\n",
            "id\nS3\n..\\bucket\nF|x\n",
            "id\nS3\nbucket\nF|/\n",
            "id\nS3\nbucket\nF|a\rb\n",
        ] {
            assert!(Selection::parse(text).is_err());
        }
        assert!(Selection::parse(&format!("id\nS3\nbucket\n{}", "F|x\n".repeat(1001))).is_err());
        Ok(())
    }
}
