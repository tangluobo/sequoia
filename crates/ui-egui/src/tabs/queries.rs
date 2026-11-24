//! Saved-query identity and dirty state without retaining a second SQL document.
use tomato_core::queries::Scope;
pub struct Document {
    pub namespace: String,
    pub scope: Scope,
    pub name: Option<String>,
    pub stamp: Option<String>,
    current: String,
    baseline: String,
    pub loading: bool,
    pub loaded: bool,
    pub saving: bool,
    pub close_after_save: bool,
    pub resave: bool,
    pub generation: u64,
}
impl Document {
    pub fn new(scope: Scope, text: &str) -> Self {
        Self {
            namespace: scope.database.clone(),
            scope,
            name: None,
            stamp: None,
            current: tomato_services::queries::stamp(text.as_bytes()),
            baseline: tomato_services::queries::stamp(b""),
            loading: false,
            loaded: false,
            saving: false,
            close_after_save: false,
            resave: false,
            generation: 0,
        }
    }
    pub fn changed(&mut self, text: &str) {
        self.current = tomato_services::queries::stamp(text.as_bytes());
    }
    pub fn dirty(&self) -> bool {
        self.current != self.baseline
    }
    pub fn loaded(&mut self, name: String, stamp: String) {
        self.name = Some(name);
        self.current = stamp.clone();
        self.baseline = stamp.clone();
        self.stamp = Some(stamp);
        self.loading = false;
        self.loaded = true;
    }
    pub fn saved(&mut self, scope: Scope, name: String, stamp: String) {
        self.scope = scope;
        self.name = Some(name);
        self.baseline = stamp.clone();
        self.stamp = Some(stamp);
        self.saving = false;
        self.loaded = true;
    }
    pub fn title(&self) -> String {
        format!(
            "{}{}",
            if self.name.is_none() || self.dirty() {
                "*"
            } else {
                ""
            },
            self.name.as_deref().unwrap_or("未保存查询")
        )
    }
}
