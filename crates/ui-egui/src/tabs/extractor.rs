use std::{collections::BTreeSet, sync::Arc};
use tomato_services::extractor::{Options, Report};
pub const CATEGORIES: &[(&str, &str, &[&str])] = &[
    (
        "GFX",
        "图像 (GFX)",
        &[
            "png", "jpg", "bmp", "gif", "ico", "cur", "dds", "tif", "emf", "wmf",
        ],
    ),
    (
        "MUSIC",
        "音频 (MUSIC)",
        &["wav", "mp3", "ogg", "mid", "xm", "mod", "s3m"],
    ),
    (
        "VIDEO",
        "视频 (VIDEO)",
        &[
            "avi", "mov", "mpg", "mp4", "3gp", "bik", "smk", "swf", "asf",
        ],
    ),
    ("DOCUMENTS", "文档 (DOCUMENTS)", &["pdf", "doc", "rtf"]),
    ("FONTS", "字体 (FONTS)", &["ttf", "otf", "ttc", "woff"]),
    (
        "ARCHIVE",
        "压缩包 (ARCHIVE)",
        &["zip", "rar", "7z", "gz", "bz2", "cab", "jar", "chm"],
    ),
    (
        "OTHER",
        "其他 (OTHER)",
        &["3ds", "upk", "iff", "dat", "mpq"],
    ),
];
pub struct State {
    pub options: Options,
    pub categories: BTreeSet<String>,
    pub selected: BTreeSet<String>,
    pub report: Option<Arc<Report>>,
    pub show_log: bool,
    pub status: String,
    pub rows: Vec<String>,
    pub selected_row: Option<usize>,
    pub scroll_to_last: bool,
    pub rows_width: f32,
}
impl Default for State {
    fn default() -> Self {
        Self {
            options: Options::default(),
            categories: Default::default(),
            selected: CATEGORIES
                .iter()
                .flat_map(|(_, _, formats)| formats.iter().map(|s| (*s).into()))
                .collect(),
            report: None,
            show_log: false,
            status: String::new(),
            rows: Vec::new(),
            selected_row: None,
            scroll_to_last: false,
            rows_width: 0.,
        }
    }
}
impl State {
    pub fn request(&self, source: &str, output: &str) -> Options {
        let mut options = self.options.clone();
        options.source = source.trim().into();
        options.output = output.trim().into();
        options.categories = self.categories.iter().cloned().collect();
        options.extensions = CATEGORIES
            .iter()
            .filter(|(id, _, _)| self.categories.contains(*id))
            .flat_map(|(_, _, formats)| formats.iter())
            .filter(|s| self.selected.contains(**s))
            .map(|s| (*s).into())
            .collect();
        options
    }
    pub fn accept(&mut self, report: Report) {
        self.rows = report
            .hits
            .iter()
            .map(|hit| {
                let source = hit
                    .entry
                    .as_deref()
                    .map(std::borrow::Cow::Borrowed)
                    .unwrap_or_else(|| {
                        hit.source.file_name().unwrap_or_default().to_string_lossy()
                    });
                format!(
                    "[{:<4}] 0x{:08X}  {:<10}  {}",
                    hit.extension,
                    hit.offset,
                    human(hit.size),
                    source
                )
            })
            .collect();
        if report.total > report.hits.len() {
            self.rows.push(format!(
                "... 还有 {} 个结果",
                report.total - report.hits.len()
            ));
        }
        self.selected_row = None;
        self.scroll_to_last = !self.rows.is_empty();
        self.rows_width = 0.;
        self.status = report.status.clone();
        self.report = Some(Arc::new(report));
    }
    pub fn begin(&mut self) {
        self.report = None;
        self.rows.clear();
        self.selected_row = None;
        self.scroll_to_last = false;
        self.rows_width = 0.;
        self.status = "正在运行...".into();
    }
}
fn human(size: u64) -> String {
    match size {
        0..1024 => format!("{size} B"),
        1024..1_048_576 => format!("{:.1} KB", size as f64 / 1024.),
        1_048_576..1_073_741_824 => format!("{:.1} MB", size as f64 / 1_048_576.),
        _ => format!("{:.2} GB", size as f64 / 1_073_741_824.),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn result_text_keeps_original_left_padding_units_and_overflow_row() {
        let mut state = State::default();
        state.accept(Report {
            hits: vec![tomato_services::extractor::Hit {
                source: "fixture.bin".into(),
                offset: 0xfffa,
                size: 1536,
                extension: "png".into(),
                entry: Some("目录/icon.png".into()),
                output: None,
            }],
            total: 501,
            ..Default::default()
        });
        assert_eq!(
            state.rows,
            [
                "[png ] 0x0000FFFA  1.5 KB      目录/icon.png",
                "... 还有 500 个结果"
            ]
        );
        assert!(state.scroll_to_last && state.selected_row.is_none());
    }
    #[test]
    fn category_filters_preserve_specific_selections_and_all_unchecked_means_all_formats() {
        let mut state = State::default();
        let all = state.request("fixture", "out");
        assert!(all.categories.is_empty() && all.extensions.is_empty());
        state.categories.insert("GFX".into());
        state.selected.remove("png");
        let selected = state.request("fixture", "out");
        assert_eq!(selected.categories, ["GFX"]);
        assert!(!selected.extensions.contains(&"png".into()));
        assert!(selected.extensions.contains(&"ico".into()));
        state.categories.clear();
        assert!(state.request("fixture", "out").extensions.is_empty());
        state.categories.insert("GFX".into());
        assert!(
            !state
                .request("fixture", "out")
                .extensions
                .contains(&"png".into())
        );
    }
}
