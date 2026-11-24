//! Opt-in startup diagnostics, restricted to graphics and window initialization.
struct Diagnostics(std::time::Instant);
impl log::Log for Diagnostics {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
            && ["wgpu", "egui_wgpu", "eframe"]
                .iter()
                .any(|prefix| metadata.target().starts_with(prefix))
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!(
                "{:.3}ms {} {}: {}",
                self.0.elapsed().as_secs_f64() * 1000.,
                record.level(),
                record.target(),
                record.args()
            );
        }
    }
    fn flush(&self) {}
}
static DIAGNOSTICS: std::sync::OnceLock<Diagnostics> = std::sync::OnceLock::new();
pub fn install() {
    if std::env::var_os("TOMATO_RENDER_DIAGNOSTICS").is_some()
        && log::set_logger(DIAGNOSTICS.get_or_init(|| Diagnostics(std::time::Instant::now())))
            .is_ok()
    {
        log::set_max_level(log::LevelFilter::Debug);
    }
}
