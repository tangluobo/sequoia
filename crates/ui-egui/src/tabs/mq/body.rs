use super::State;
impl State {
    /// Original JsonFoldableTextView formats once when a message is opened.
    pub fn set_body(&mut self, mut text: String) {
        if text.trim_matches(|ch: char| ch <= ' ').is_empty() {
            text = String::new();
        }
        self.body_container = false;
        let mut structured = false;
        if !text.is_empty()
            && let Ok(value) =
                tomato_core::tools::json_parse(text.trim_matches(|ch: char| ch <= ' '))
        {
            match tomato_core::tools::json_write(&value, true) {
                Ok(formatted) => {
                    text = formatted;
                    structured = true;
                    self.body_container = value.is_object() || value.is_array();
                }
                Err(error) => {
                    text = format!("消息正文显示失败：{error}");
                }
            }
        }
        if !structured && text.contains('\r') {
            // Parse first, preserving CR inside Gson strings. RichTextFX only
            // normalizes physical line endings after falling back to plain text.
            let mut bytes = text.into_bytes();
            let (mut read, mut write) = (0, 0);
            while read < bytes.len() {
                if bytes[read] == b'\r' {
                    bytes[write] = b'\n';
                    read += 1;
                    if bytes.get(read) == Some(&b'\n') {
                        read += 1;
                    }
                } else {
                    bytes[write] = bytes[read];
                    read += 1;
                }
                write += 1;
            }
            bytes.truncate(write);
            text =
                String::from_utf8(bytes).expect("normalizing ASCII line endings preserves UTF-8");
        }
        self.body = text;
        self.body_view.reset(&self.body, structured);
        self.body_reset = true;
    }
    pub fn format_body(&mut self, pretty: bool) {
        if !self.body_container {
            return;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.body)
            && let Ok(text) = tomato_core::tools::json_write(&value, pretty)
        {
            self.body = text;
            self.body_view.reset(&self.body, true);
            self.body_reset = true;
        }
    }
    pub fn copy_body(&self) -> String {
        self.body_view.visible_text(&self.body)
    }
}
