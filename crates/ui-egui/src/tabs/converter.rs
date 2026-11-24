//! One conversion belongs to its tab; dropping the tab cancels queued/disk work.
use tomato_services::{
    backup::Control,
    commands::Command,
    tools::conversion::{Report, Request},
};
pub struct State {
    control: Option<Control>,
    pub status: String,
    pub error: bool,
    pub focus_source: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            control: None,
            status: String::new(),
            error: false,
            focus_source: true,
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl State {
    fn cancel(&mut self) {
        if let Some(control) = self.control.take() {
            control.cancel();
        }
    }
    pub fn running(&self) -> bool {
        self.control.is_some()
    }
    pub fn start(&mut self, dataset: bool, source: &str, destination: &str) -> Option<Command> {
        let source = source.trim_matches(|c: char| c <= ' ');
        let destination = destination.trim_matches(|c: char| c <= ' ');
        self.error = true;
        if source.is_empty() {
            self.status = if dataset {
                "请选择源数据集目录"
            } else {
                "请选择源文件或目录"
            }
            .into();
            return None;
        }
        if destination.is_empty() {
            self.status = if dataset {
                "请选择目标数据集目录"
            } else {
                "请选择输出目录"
            }
            .into();
            return None;
        }
        self.cancel();
        let control = Control::default();
        self.control = Some(control.clone());
        self.status = "正在转换...".into();
        self.error = false;
        Some(Command::ConvertTool {
            request: if dataset {
                Request::Dataset {
                    source: source.into(),
                    directory: destination.into(),
                }
            } else {
                Request::Images {
                    source: source.into(),
                    directory: destination.into(),
                    width: 0,
                }
            },
            control: Some(control),
        })
    }
    pub fn accept(&mut self, report: Report) {
        self.control = None;
        self.status = report.message();
        self.error = report.has_errors();
    }
    pub fn fail(&mut self, message: &str) {
        self.fail_input(&format!("转换失败：{message}"));
    }
    pub fn fail_input(&mut self, message: &str) {
        self.cancel();
        self.status = message.into();
        self.error = true;
    }
}
