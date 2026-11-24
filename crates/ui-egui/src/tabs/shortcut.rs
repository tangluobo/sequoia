use std::path::PathBuf;
pub struct State {
    pub fields: tomato_services::tools::shortcut::Shortcut,
    pub tab: usize,
    pub directory: String,
    pub file_name: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            fields: tomato_services::tools::shortcut::Shortcut {
                category: "Utility".into(),
                notify: false,
                ..Default::default()
            },
            tab: 0,
            directory: std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".local/share/applications")
                .display()
                .to_string(),
            file_name: String::new(),
        }
    }
}
