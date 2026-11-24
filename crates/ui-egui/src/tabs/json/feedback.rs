//! Original JSON tool feedback. Successful messages expire without a repaint loop.
use std::time::{Duration, Instant};
use tomato_services::{commands::Command, json_history::Action};
#[derive(Clone, Copy)]
pub struct Feedback {
    pub success: Option<&'static str>,
    pub format_error: bool,
}
impl Feedback {
    pub fn for_command(command: &Command) -> Option<Self> {
        let (success, format_error) = match command {
            Command::Json { action, .. } => (
                Some(match action.as_str() {
                    "format" => "格式化成功！",
                    "minify" => "压缩成功！",
                    "unicode" => "中文转Unicode成功！",
                    "decode_unicode" => "Unicode转中文成功！",
                    "escape" => "添加转义成功！",
                    "unescape" => "去除转义成功！",
                    _ => return None,
                }),
                matches!(action.as_str(), "format" | "minify"),
            ),
            Command::JsonHistory { action, .. } => match action {
                Action::Format { .. } => (Some("格式化成功！"), true),
                Action::Restore(_) => (Some("已从历史恢复！"), true),
                Action::Copy(_) => (Some("已复制到剪贴板！"), false),
                Action::Clear => (Some("历史记录已清空"), false),
                Action::Remove(_) => (None, false),
                Action::Load => return None,
            },
            _ => return None,
        };
        Some(Self {
            success,
            format_error,
        })
    }
}
#[derive(Default)]
pub struct Status {
    pub text: String,
    pub error: bool,
    pub until: Option<Instant>,
}
impl Status {
    pub fn success(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.error = false;
        self.until = Some(Instant::now() + Duration::from_secs(3));
    }
    pub fn error(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.error = true;
        self.until = None;
    }
    pub fn expire(&mut self) -> Option<Duration> {
        let remaining = self.until?.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            self.text.clear();
            self.until = None;
            None
        } else {
            Some(remaining)
        }
    }
}
