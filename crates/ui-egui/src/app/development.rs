use super::*;
impl TomatoApp {
    pub(super) fn ensure_development(&mut self) {
        if self.development.is_none()
            && let Some(runtime) = &self.runtime
        {
            let path = self
                .repository
                .location()
                .map(|p| p.with_file_name("development.json"))
                .unwrap_or_else(|| Store::default_path().with_file_name("development.json"));
            self.development = Some(Box::new(crate::development::State::new(
                runtime,
                &self.context,
                path,
            )));
        }
    }
}
