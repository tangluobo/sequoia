use super::*;
use tomato_core::development::{
    BuildTool, Runtime,
    runtime::{self, EntryRole},
};
pub(super) fn next_epoch() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}
#[derive(Clone, Copy)]
pub(crate) struct EntryEditor {
    pub role: EntryRole,
    pub index: Option<usize>,
    pub epoch: u64,
}
pub(crate) struct Draft {
    pub index: usize,
    pub config: Config,
    pub tab: usize,
    pub edit: Option<EntryEditor>,
    pub kind: String,
    pub name: String,
    pub path: String,
    pub version: String,
    pub settings: String,
    pub selected_runtime: Option<usize>,
    pub selected_tool: Option<usize>,
    pub warning: Option<String>,
    pub warning_focus: bool,
    pub focus_main: bool,
    pub focus_entry: bool,
    pub fields: [Editor; 4],
    context: Option<egui::Context>,
    auto_name: Option<String>,
    sequence: u64,
}
impl Draft {
    pub fn new(index: usize, config: &Config) -> Self {
        let mut value = Self {
            index,
            config: config.clone(),
            tab: 0,
            edit: None,
            kind: "JDK".into(),
            name: String::new(),
            path: String::new(),
            version: String::new(),
            settings: String::new(),
            selected_runtime: None,
            selected_tool: None,
            warning: None,
            warning_focus: false,
            focus_main: true,
            focus_entry: false,
            fields: Default::default(),
            context: None,
            auto_name: None,
            sequence: 0,
        };
        value.normalize();
        value
    }
    fn normalize(&mut self) {
        let p = &mut self.config.projects[self.index];
        for (selected, kind) in [
            (&mut p.jdk_id, "JDK"),
            (&mut p.node_id, "NODE"),
            (&mut p.flutter_id, "FLUTTER"),
        ] {
            if !self
                .config
                .runtimes
                .iter()
                .any(|v| v.id == *selected && v.kind == kind)
            {
                selected.clear();
            }
        }
        let kind = runtime::build_kind(&p.project_type);
        if !self
            .config
            .build_tools
            .iter()
            .any(|v| v.id == p.maven_id && Some(v.kind.as_str()) == kind)
        {
            p.maven_id.clear();
        }
    }
    pub fn begin(&mut self, role: EntryRole, index: Option<usize>) {
        self.release_fields();
        for field in &mut self.fields {
            field.reset();
        }
        self.sequence = next_epoch();
        self.edit = Some(EntryEditor {
            role,
            index,
            epoch: self.sequence,
        });
        self.warning = None;
        self.focus_entry = true;
        self.auto_name = None;
        if role.is_build()
            && let Some(value) = index.and_then(|i| self.config.build_tools.get(i))
        {
            self.kind = value.kind.clone();
            self.name = value.name.clone();
            self.path = value.home_path.clone();
            self.version = value.version.clone();
            self.settings = value.settings_path.clone();
        } else if !role.is_build()
            && let Some(value) = index.and_then(|i| self.config.runtimes.get(i))
        {
            self.kind = value.kind.clone();
            self.name = value.name.clone();
            self.path = value.path.clone();
            self.version = value.version.clone();
            self.settings.clear();
        } else {
            self.kind = if role.is_build() { "MAVEN" } else { "JDK" }.into();
            self.name.clear();
            self.path.clear();
            self.version.clear();
            self.settings.clear();
        }
    }
    pub fn invalidate_detection(&mut self) {
        self.sequence = next_epoch();
        if let Some(edit) = &mut self.edit {
            edit.epoch = self.sequence;
        }
        self.auto_name = None;
    }
    pub fn change_kind(&mut self) {
        for field in &mut self.fields {
            field.reset();
        }
        self.invalidate_detection();
        self.name.clear();
        self.path.clear();
        self.version.clear();
        self.settings.clear();
    }
    pub fn name_changed(&mut self) {
        self.auto_name = None;
    }
    pub fn browsed(&mut self, path: PathBuf) {
        self.fields[1].reset();
        self.fields[2].reset();
        self.fields[3].reset();
        self.invalidate_detection();
        self.path = path.to_string_lossy().into_owned();
        if self.edit.is_some_and(|e| e.role.is_build()) {
            self.settings = if self.kind == "MAVEN" {
                path.join("conf/settings.xml")
                    .to_string_lossy()
                    .into_owned()
            } else {
                String::new()
            };
        }
        if self.name.trim().is_empty() {
            let name = runtime::display_kind(&self.kind).to_owned();
            self.name = name.clone();
            self.auto_name = Some(name);
        }
    }
    pub fn detected(&mut self, version: String) {
        self.fields[2].reset();
        if self.auto_name.take().is_some_and(|name| name == self.name) {
            self.fields[0].reset();
            self.name = format!(
                "{}{}{}",
                runtime::display_kind(&self.kind),
                if version.is_empty() { "" } else { " " },
                version
            );
        }
        self.version = version;
    }
    pub fn remove(&mut self, role: EntryRole) {
        let selected = if role.is_build() {
            &mut self.selected_tool
        } else {
            &mut self.selected_runtime
        };
        if let Some(index) = *selected {
            let count = if role.is_build() {
                if index >= self.config.build_tools.len() {
                    return;
                };
                self.config.build_tools.remove(index);
                self.config.build_tools.len()
            } else {
                if index >= self.config.runtimes.len() {
                    return;
                };
                self.config.runtimes.remove(index);
                self.config.runtimes.len()
            };
            *selected = if count == 0 {
                None
            } else {
                Some(index.min(count - 1))
            };
            self.normalize();
        }
    }
    pub fn accept(&mut self) {
        let Some(edit) = self.edit else { return };
        if edit.role.is_build() {
            let mut value = edit
                .index
                .and_then(|i| self.config.build_tools.get(i))
                .cloned()
                .unwrap_or_else(|| BuildTool {
                    id: tomato_core::Connection::new("BUILD").id,
                    ..Default::default()
                });
            value.kind = self.kind.clone();
            value.name = self.name.trim().into();
            value.home_path = self.path.trim().into();
            value.version = self.version.trim().into();
            value.settings_path = self.settings.trim().into();
            if let Some(slot) = edit.index.and_then(|i| self.config.build_tools.get_mut(i)) {
                *slot = value;
            } else {
                self.config.build_tools.push(value);
            }
        } else {
            let mut value = edit
                .index
                .and_then(|i| self.config.runtimes.get(i))
                .cloned()
                .unwrap_or_else(|| Runtime {
                    id: tomato_core::Connection::new("SDK").id,
                    ..Default::default()
                });
            value.kind = self.kind.clone();
            value.name = self.name.trim().into();
            value.path = self.path.trim().into();
            value.version = self.version.trim().into();
            if let Some(slot) = edit.index.and_then(|i| self.config.runtimes.get_mut(i)) {
                *slot = value;
            } else {
                self.config.runtimes.push(value);
            }
        }
        self.close_entry();
        self.normalize();
    }
    pub fn bind(&mut self, ctx: &egui::Context) {
        self.context = Some(ctx.clone());
    }
    pub fn close_entry(&mut self) {
        self.release_fields();
        self.edit = None;
        self.warning = None;
    }
    fn release_fields(&self) {
        if let Some(ctx) = &self.context {
            for field in &self.fields {
                field.release(ctx);
            }
        }
    }
}
impl Drop for Draft {
    fn drop(&mut self) {
        self.release_fields();
    }
}
