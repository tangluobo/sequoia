use super::*;
impl Workspace {
    pub fn paste_objects(&mut self, ctx: &egui::Context, directory: String) {
        if self.object_copy.is_some() {
            self.panes.entry(Section::Files).or_default().status =
                "已有对象复制任务正在执行".into();
            return;
        }
        let job = std::sync::Arc::new(tomato_services::files::object_copy::Job::default());
        let context = ctx.clone();
        job.on_change(move || context.request_repaint());
        let before = self.queued.len();
        self.enqueue(
            Section::Files,
            Operation::PasteObjects {
                directory,
                job: job.clone(),
            },
        );
        if self.queued.len() > before {
            self.object_copy = Some(tomato_services::files::object_copy::Owner(job));
            self.copy_window_visible = true;
            self.copy_window_positioned = false;
            self.copy_inner_size = None;
            self.copy_errors = None;
        }
    }
}
