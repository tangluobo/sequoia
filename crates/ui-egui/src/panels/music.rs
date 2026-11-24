use super::{Intent, tools};
use crate::theme;
use egui::{Button, Color32, Grid, Label, RichText, ScrollArea, TextEdit, Ui};
use std::path::PathBuf;
use tomato_services::{
    backup::Control,
    commands::Command,
    tools::music::{
        CatalogTrack, Download, Output, Playback, Request, SearchBy, SearchScope, Track,
    },
};

pub(crate) const DOWNLOAD_DIRECTORY: &str = "musicDownloadDirectory";
const ROW_HEIGHT: f32 = 34.0;

#[derive(Default)]
pub(crate) struct State {
    query: String,
    by: SearchBy,
    source: SearchSource,
    submitted_query: String,
    submitted_by: SearchBy,
    submitted_source: SearchSource,
    directory: String,
    saved_connection: bool,
    rows: Vec<Row>,
    catalog: Option<Vec<CatalogTrack>>,
    page: Option<u32>,
    more: bool,
    unavailable: usize,
    summary: String,
    pending: Option<(Operation, Control)>,
    playing: Option<(usize, Playback)>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum SearchSource {
    Archive,
    #[default]
    Public,
    Catalog,
}

impl SearchSource {
    fn label(self) -> &'static str {
        match self {
            Self::Archive => "现场 FLAC",
            Self::Public => "公开 FLAC",
            Self::Catalog => "歌曲资料（查歌）",
        }
    }

    fn search(self, by: SearchBy, query: String, page: u32) -> Request {
        let scope = match self {
            Self::Catalog => return Request::CatalogSearch { query },
            Self::Public => SearchScope::Public,
            Self::Archive => SearchScope::Live,
        };
        Request::Search {
            by,
            query,
            page,
            scope,
        }
    }
}

struct Row {
    track: Track,
    selected: bool,
    download: Option<Download>,
}

enum Operation {
    Search,
    Download(Vec<usize>),
    Preview(usize),
}

enum RowAction {
    Play(usize),
    Stop,
    Download(usize),
}

impl State {
    pub(crate) fn set_directory(&mut self, directory: String) {
        self.directory = directory;
        self.saved_connection = true;
    }

    pub(crate) fn accept(&mut self, result: Output) {
        let operation = self.pending.take().map(|(operation, _)| operation);
        match result {
            Output::Search {
                tracks,
                page,
                more,
                unavailable,
            } => {
                self.playing = None;
                self.catalog = None;
                self.rows = tracks
                    .into_iter()
                    .map(|track| Row {
                        track,
                        selected: false,
                        download: None,
                    })
                    .collect();
                self.page = Some(page);
                self.more = more;
                self.unavailable = unavailable;
            }
            Output::Catalog { tracks } => {
                self.playing = None;
                self.rows.clear();
                self.page = None;
                self.catalog = Some(tracks);
            }
            Output::Download {
                outcomes,
                cancelled,
            } => {
                let Some(Operation::Download(indices)) = operation else {
                    return;
                };
                let saved = outcomes
                    .iter()
                    .filter(|outcome| matches!(outcome, Download::Saved(_)))
                    .count();
                let failed = outcomes.len() - saved;
                for (index, outcome) in indices.into_iter().zip(outcomes) {
                    let row = &mut self.rows[index];
                    if matches!(outcome, Download::Saved(_)) {
                        row.selected = false;
                    }
                    row.download = Some(outcome);
                }
                let status = if cancelled {
                    "已取消"
                } else {
                    "下载结束"
                };
                self.summary = format!("{status} · 已保存 {saved} 首 · 失败 {failed} 首");
            }
            Output::Preview { playback } => {
                if let Some(Operation::Preview(index)) = operation {
                    self.playing = Some((index, playback));
                }
            }
        }
    }

    fn search(&mut self, ui: &Ui, page: Option<u32>) -> Intent {
        self.playing = None;
        if page.is_none() {
            self.submitted_query = self.query.trim().to_owned();
            self.submitted_by = self.by;
            self.submitted_source = self.source;
            self.page = None;
            self.rows.clear();
            self.catalog = None;
        }
        self.summary.clear();
        let request = self.submitted_source.search(
            self.submitted_by,
            self.submitted_query.clone(),
            page.unwrap_or(0),
        );
        self.command(ui, Operation::Search, request)
    }

    fn download(&mut self, ui: &Ui, indices: Vec<usize>) -> Intent {
        let tracks = indices
            .iter()
            .map(|&index| self.rows[index].track.clone())
            .collect();
        self.summary.clear();
        self.command(
            ui,
            Operation::Download(indices),
            Request::Download {
                tracks,
                directory: PathBuf::from(self.directory.trim()),
            },
        )
    }

    fn preview(&mut self, ui: &Ui, index: usize) -> Intent {
        self.playing = None;
        let row = &self.rows[index];
        let local_file = match &row.download {
            Some(Download::Saved(path)) => Some(path.clone()),
            _ => None,
        };
        self.command(
            ui,
            Operation::Preview(index),
            Request::Preview {
                track: row.track.clone(),
                local_file,
            },
        )
    }

    fn command(&mut self, ui: &Ui, operation: Operation, request: Request) -> Intent {
        let ctx = ui.ctx().clone();
        let control = Control::new(move || ctx.request_repaint());
        self.pending = Some((operation, control.clone()));
        Intent::command(Command::Music {
            request,
            control: Some(control),
        })
    }
}

impl Drop for State {
    fn drop(&mut self) {
        if let Some((_, control)) = &self.pending {
            control.cancel();
        }
    }
}

pub(super) fn render(
    ui: &mut Ui,
    state: &mut State,
    busy: bool,
    icons: &mut crate::icons::Icons,
) -> Option<Intent> {
    tools::header(
        ui,
        icons,
        "music_downloader",
        "无损音乐下载",
        "公开 FLAC 与华语曲目",
    );
    if !busy {
        state.pending = None;
    }
    if state
        .playing
        .as_ref()
        .is_some_and(|(_, player)| !player.is_active())
    {
        state.playing = None;
    }
    let mut intent = None;
    egui::Frame::new().inner_margin(14).show(ui, |ui| {
        egui::Frame::new()
            .fill(Color32::from_rgb(247, 249, 248))
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(224, 233, 227)))
            .corner_radius(8)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.add_enabled_ui(!busy, |ui| {
                    intent = search_controls(ui, state);
                    if let Some(configure) = directory_controls(ui, state) {
                        intent = Some(configure);
                    }
                });
                progress(ui, state, busy);
            });
        ui.add_space(12.0);
        if let Some(tracks) = &state.catalog {
            catalog_results(ui, tracks);
            return;
        }
        if let Some(page) = state.page {
            let next = ui
                .add_enabled_ui(!busy, |ui| results_controls(ui, state, page))
                .inner;
            if next.is_some() {
                intent = next;
            }
        }
        if let Some(index) = state.playing.as_ref().map(|(index, _)| *index) {
            ui.horizontal(|ui| {
                if ui.button("停止").clicked() {
                    state.playing = None;
                }
                ui.label(
                    RichText::new("正在播放")
                        .color(Color32::from_rgb(22, 113, 72))
                        .strong(),
                );
                ui.add(Label::new(&state.rows[index].track.title).truncate());
            });
        }
        if state.unavailable > 0 && state.page.is_some() {
            ui.label(format!("有 {} 条录音的曲目暂时无法读取", state.unavailable));
        }
        if state.rows.is_empty() {
            ui.add_space(24.0);
            if state.page.is_some() || !busy {
                ui.vertical_centered(|ui| {
                    ui.label(if state.page.is_some() {
                        "这一页没有匹配的 FLAC 歌曲"
                    } else {
                        "输入歌名或歌手开始搜索"
                    });
                });
            }
        } else {
            let playing = state.playing.as_ref().map(|(index, _)| *index);
            let can_download = !state.directory.trim().is_empty();
            let row_action = table(ui, &mut state.rows, busy, playing, can_download);
            match row_action {
                Some(RowAction::Play(index)) => intent = Some(state.preview(ui, index)),
                Some(RowAction::Stop) => state.playing = None,
                Some(RowAction::Download(index)) => intent = Some(state.download(ui, vec![index])),
                None => {}
            }
        }
    });
    intent
}

fn search_controls(ui: &mut Ui, state: &mut State) -> Option<Intent> {
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("music-source")
            .selected_text(state.source.label())
            .show_ui(ui, |ui| {
                for source in [
                    SearchSource::Public,
                    SearchSource::Archive,
                    SearchSource::Catalog,
                ] {
                    ui.selectable_value(&mut state.source, source, source.label());
                }
            });
        ui.add_enabled_ui(state.source != SearchSource::Catalog, |ui| {
            ui.selectable_value(&mut state.by, SearchBy::Any, "全部");
            ui.selectable_value(&mut state.by, SearchBy::Song, "歌名");
            ui.selectable_value(&mut state.by, SearchBy::Artist, "歌手");
        });
        let input = ui.add(
            TextEdit::singleline(&mut state.query)
                .hint_text("输入歌名或歌手")
                .char_limit(100)
                .desired_width(240.0),
        );
        let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let valid = ui.is_enabled() && !state.query.trim().is_empty();
        let clicked = ui.add_enabled(valid, theme::green_button("搜索")).clicked();
        (clicked || (valid && enter)).then(|| state.search(ui, None))
    })
    .inner
}

fn directory_controls(ui: &mut Ui, state: &mut State) -> Option<Intent> {
    ui.horizontal(|ui| {
        ui.label("下载目录");
        if state.saved_connection {
            let path = if state.directory.is_empty() {
                "请选择下载目录"
            } else {
                &state.directory
            };
            ui.add_sized(
                [ui.available_width().clamp(80.0, 380.0) - 52.0, 24.0],
                Label::new(path).truncate(),
            )
            .on_hover_text(path);
            if ui.button("设置…").clicked() {
                return Some(Intent::ConfigureMusic);
            }
        } else {
            ui.add(
                TextEdit::singleline(&mut state.directory)
                    .hint_text("选择文件夹")
                    .desired_width(260.0),
            );
            if ui.button("浏览…").clicked()
                && let Some(directory) = rfd::FileDialog::new()
                    .set_title("选择音乐下载目录")
                    .pick_folder()
            {
                state.directory = directory.to_string_lossy().into_owned();
            }
        }
        None
    })
    .inner
}

fn results_controls(ui: &mut Ui, state: &mut State, page: u32) -> Option<Intent> {
    ui.horizontal_wrapped(|ui| {
        ui.strong(format!("{} 首", state.rows.len()));
        let indices: Vec<_> = state
            .rows
            .iter()
            .enumerate()
            .filter_map(|(index, row)| row.selected.then_some(index))
            .collect();
        let label = format!("下载所选（{}）", indices.len());
        if ui
            .add_enabled(
                !indices.is_empty() && !state.directory.trim().is_empty(),
                theme::green_button(&label),
            )
            .clicked()
        {
            return Some(state.download(ui, indices));
        }
        ui.separator();
        let same_query = state.submitted_query == state.query.trim()
            && state.submitted_by == state.by
            && state.submitted_source == state.source;
        if ui
            .add_enabled(same_query && page > 0, Button::new("上一页"))
            .clicked()
        {
            return Some(state.search(ui, Some(page - 1)));
        }
        ui.label(format!("第 {} 页", page + 1));
        if ui
            .add_enabled(same_query && state.more, Button::new("下一页"))
            .clicked()
        {
            return Some(state.search(ui, Some(page + 1)));
        }
        None
    })
    .inner
}

fn catalog_results(ui: &mut Ui, tracks: &[CatalogTrack]) {
    ui.label("歌曲资料提供官方详情链接。播放和下载请查询 FLAC 音源。");
    if tracks.is_empty() {
        ui.label("Apple Music 华语目录没有找到匹配的曲目。");
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.strong(format!("{} 首", tracks.len()));
        ui.label("Apple Music 华语目录 · 曲目详情");
    });
    let gap = ui.spacing().item_spacing;
    let album_width = (ui.available_width() - 470.0).clamp(120.0, 360.0);
    ScrollArea::both().auto_shrink([false, false]).show_rows(
        ui,
        ROW_HEIGHT,
        tracks.len() + 1,
        |ui, range| {
            Grid::new("music-catalog")
                .start_row(range.start)
                .striped(true)
                .min_row_height(ROW_HEIGHT)
                .spacing(gap)
                .show(ui, |ui| {
                    let widths = [180.0, 130.0, album_width, 110.0];
                    for index in range {
                        if index == 0 {
                            for (label, width) in
                                ["歌曲", "歌手", "专辑", "来源"].into_iter().zip(widths)
                            {
                                cell(ui, width, RichText::new(label).strong());
                            }
                        } else {
                            let song = &tracks[index - 1];
                            cell(ui, widths[0], RichText::new(&song.title).strong())
                                .on_hover_text(&song.title);
                            cell(ui, widths[1], &song.artist).on_hover_text(&song.artist);
                            cell(ui, widths[2], &song.album).on_hover_text(&song.album);
                            ui.hyperlink_to("打开 Apple Music", &song.page);
                        }
                        ui.end_row();
                    }
                });
        },
    );
}

fn progress(ui: &mut Ui, state: &State, busy: bool) {
    if busy && let Some((operation, control)) = &state.pending {
        ui.horizontal(|ui| {
            ui.spinner();
            match operation {
                Operation::Search => ui.label("正在搜索…"),
                Operation::Preview(_) => ui.label("正在载入音频…"),
                Operation::Download(indices) => {
                    let done = control.read(|p| p.objects).unwrap_or(0);
                    ui.label(format!("正在下载（{done}/{}）", indices.len()))
                }
            };
            if ui
                .add_enabled(!control.cancelled(), Button::new("取消"))
                .clicked()
            {
                control.cancel();
            }
        });
    } else if !state.summary.is_empty() {
        ui.label(&state.summary);
    }
}

fn table(
    ui: &mut Ui,
    rows: &mut [Row],
    busy: bool,
    playing: Option<usize>,
    can_download: bool,
) -> Option<RowAction> {
    let mut widths = [28.0, 150.0, 104.0, 0.0, 100.0, 48.0, 64.0];
    let gap = ui.spacing().item_spacing;
    let occupied = widths.iter().sum::<f32>() + gap.x * (widths.len() - 1) as f32;
    widths[3] = (ui.available_width() - occupied - 18.0).clamp(130.0, 420.0);
    let selectable = rows
        .iter()
        .filter(|row| !matches!(row.download, Some(Download::Saved(_))));
    let mut all = selectable.clone().all(|row| row.selected) && selectable.count() > 0;
    let mut action = None;
    ui.add_enabled_ui(!busy, |ui| {
        ScrollArea::both().auto_shrink([false, false]).show_rows(
            ui,
            ROW_HEIGHT,
            rows.len() + 1,
            |ui, range| {
                Grid::new("music-tracks")
                    .start_row(range.start)
                    .striped(true)
                    .spacing(gap)
                    .min_col_width(0.0)
                    .min_row_height(ROW_HEIGHT)
                    .show(ui, |ui| {
                        for index in range {
                            if index == 0 {
                                if ui
                                    .add_sized(
                                        [widths[0], ROW_HEIGHT],
                                        egui::Checkbox::without_text(&mut all),
                                    )
                                    .on_hover_text("全选这一页")
                                    .changed()
                                {
                                    for row in rows.iter_mut() {
                                        row.selected = all
                                            && !matches!(row.download, Some(Download::Saved(_)));
                                    }
                                }
                                for (title, width) in
                                    ["歌曲", "歌手", "录音", "格式 / 大小", "播放", "下载"]
                                        .into_iter()
                                        .zip(widths[1..].iter())
                                {
                                    cell(ui, *width, RichText::new(title).strong());
                                }
                            } else {
                                let index = index - 1;
                                let row = &mut rows[index];
                                let saved = matches!(row.download, Some(Download::Saved(_)));
                                ui.add_enabled_ui(!saved, |ui| {
                                    ui.add_sized(
                                        [widths[0], ROW_HEIGHT],
                                        egui::Checkbox::without_text(&mut row.selected),
                                    )
                                });
                                cell(ui, widths[1], RichText::new(&row.track.title).strong())
                                    .on_hover_text(&row.track.title);
                                cell(ui, widths[2], &row.track.artist)
                                    .on_hover_text(&row.track.artist);
                                cell(ui, widths[3], &row.track.recording)
                                    .on_hover_text(&row.track.recording);
                                cell(
                                    ui,
                                    widths[4],
                                    format!(
                                        "FLAC · {:.1} MiB",
                                        row.track.bytes as f64 / (1024.0 * 1024.0)
                                    ),
                                );
                                let active = playing == Some(index);
                                if ui
                                    .add_sized(
                                        [widths[5], 26.0],
                                        Button::new(if active { "停止" } else { "播放" }),
                                    )
                                    .clicked()
                                {
                                    action = Some(if active {
                                        RowAction::Stop
                                    } else {
                                        RowAction::Play(index)
                                    });
                                }
                                let button = match &row.download {
                                    Some(Download::Saved(_)) => "已下载",
                                    Some(Download::Failed(_)) => "重试",
                                    None => "下载",
                                };
                                let response = ui
                                    .add_enabled_ui(can_download && !saved, |ui| {
                                        ui.add_sized([widths[6], 26.0], Button::new(button))
                                    })
                                    .inner;
                                if response.clicked() {
                                    action = Some(RowAction::Download(index));
                                }
                                match &row.download {
                                    Some(Download::Saved(path)) => {
                                        response.on_disabled_hover_text(path.display().to_string());
                                    }
                                    Some(Download::Failed(error)) => {
                                        response.on_hover_text(error);
                                    }
                                    None => {}
                                }
                            }
                            ui.end_row();
                        }
                    });
            },
        );
    });
    action
}

fn cell(ui: &mut Ui, width: f32, value: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add_sized([width, ROW_HEIGHT], Label::new(value).truncate())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flac_result_buttons_submit_playback_and_single_download() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        for label in ["播放", "下载"] {
            let mut state = State::default();
            state.set_directory("fixture-output".into());
            state.accept(Output::Search {
                tracks: vec![Track {
                    item: "fixture".into(),
                    file: "Mojito.flac".into(),
                    title: "Mojito".into(),
                    artist: "周杰伦".into(),
                    recording: "录音".into(),
                    bytes: 21332722,
                    sha1: None,
                }],
                page: 0,
                more: false,
                unavailable: 0,
            });
            let mut icons = crate::icons::Icons::default();
            let mut frame = |events| {
                let mut intent = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1200.0, 720.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| intent = render(ui, &mut state, false, &mut icons),
                );
                let button = output
                    .shapes
                    .iter()
                    .rev()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == label => {
                            Some(text.pos + text.galley.size() / 2.0)
                        }
                        _ => None,
                    });
                output.textures_delta.clear();
                (intent, button)
            };
            frame(Vec::new());
            let (_, point) = frame(Vec::new());
            let point = point.expect("Visible FLAC row action");
            frame(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            let (intent, _) = frame(vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            let Some(Intent::Command(command)) = intent else {
                panic!("Expected row action from {label}");
            };
            match (*command, label) {
                (
                    Command::Music {
                        request: Request::Preview { track, .. },
                        ..
                    },
                    "播放",
                ) => {
                    assert_eq!(track.file, "Mojito.flac");
                }
                (
                    Command::Music {
                        request: Request::Download { tracks, directory },
                        ..
                    },
                    "下载",
                ) => {
                    assert_eq!(tracks.len(), 1);
                    assert_eq!(tracks[0].file, "Mojito.flac");
                    assert_eq!(directory, PathBuf::from("fixture-output"));
                }
                _ => panic!("Wrong music action from {label}"),
            }
        }
    }

    #[test]
    fn chinese_search_from_the_default_view_requires_downloadable_flac() {
        let ctx = egui::Context::default();
        let mut state = State::default();
        state.query.push_str("告白气球");
        let mut requested = None;
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            requested = Some(state.search(ui, None));
        });
        output.textures_delta.clear();
        let Some(Intent::Command(command)) = requested else {
            panic!("Expected a search command from the music panel");
        };
        assert!(matches!(
            *command,
            Command::Music {
                request: Request::Search { by: SearchBy::Any, query, scope: SearchScope::Public, .. },
                ..
            } if query == "告白气球"
        ));
    }

    #[test]
    fn single_download_updates_its_own_row_and_keeps_the_batch_selection() {
        let mut state = State::default();
        let make_track = |title: &str| Track {
            item: "recording".into(),
            file: format!("{title}.flac"),
            title: title.into(),
            artist: "artist".into(),
            recording: "session".into(),
            bytes: 1024,
            sha1: None,
        };
        state.accept(Output::Search {
            tracks: vec![make_track("first"), make_track("second")],
            page: 0,
            more: false,
            unavailable: 0,
        });
        state.rows[0].selected = true;
        state.pending = Some((Operation::Download(vec![1]), Control::default()));
        state.accept(Output::Download {
            outcomes: vec![Download::Saved(PathBuf::from("second.flac"))],
            cancelled: false,
        });
        assert!(state.rows[0].selected);
        assert!(state.rows[0].download.is_none());
        assert!(
            matches!(&state.rows[1].download, Some(Download::Saved(path)) if path == std::path::Path::new("second.flac"))
        );
    }
}
