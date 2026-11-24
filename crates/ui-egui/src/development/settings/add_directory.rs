//! Original add-directory form. All path inspection and persistence use the actor.
use super::*;
use crate::native_dialog::message::{self, Answer, Button, Kind, Message, Role};
use egui::{Color32, Rect};
enum Alert {
    Warning(String),
    Duplicate(Box<Project>),
}
pub(crate) struct Draft {
    project: Project,
    epoch: u64,
    fields: [Editor; 2],
    context: Option<egui::Context>,
    alert: Option<Alert>,
    focus_alert: bool,
    focus: bool,
}
impl Draft {
    pub fn new() -> Self {
        Self {
            project: Project::default(),
            epoch: draft::next_epoch(),
            fields: Default::default(),
            context: None,
            alert: None,
            focus_alert: true,
            focus: true,
        }
    }
    fn changed(&mut self) {
        self.epoch = draft::next_epoch();
    }
    fn warn(&mut self, text: &str) {
        self.alert = Some(Alert::Warning(text.into()));
        self.focus_alert = true;
    }
}
impl Drop for Draft {
    fn drop(&mut self) {
        if let Some(ctx) = &self.context {
            for field in &self.fields {
                field.release(ctx);
            }
        }
    }
}
fn persist(state: &mut State, project: Project) {
    let mut config = state.config.clone();
    let select = state
        .project()
        .map(|p| p.path.clone())
        .or_else(|| Some(project.path.clone()));
    config.projects.push(project);
    state.save_config(config, select, None);
}
pub(crate) fn result(state: &mut State, epoch: u64, save: bool, valid: bool, project_type: String) {
    let Some(draft) = state.add.as_mut().filter(|d| d.epoch == epoch) else {
        return;
    };
    if !save {
        draft.project.project_type = if valid {
            project_type
        } else {
            "GENERAL".into()
        };
        return;
    }
    if !valid {
        draft.warn("请选择一个有效的本地目录");
        return;
    }
    let mut project = draft.project.clone();
    project.name = project.name.trim().into();
    project.path = project.path.trim().into();
    project.project_type = project_type;
    if state
        .config
        .projects
        .iter()
        .any(|p| p.name.to_lowercase() == project.name.to_lowercase())
    {
        draft.alert = Some(Alert::Duplicate(Box::new(project)));
        draft.focus_alert = true;
    } else {
        persist(state, project);
    }
}
enum Action {
    Browse,
    Save,
    Cancel,
}
fn contents(ui: &mut Ui, draft: &mut Draft, busy: bool) -> Option<Action> {
    crate::panels::ensure_bold_font(ui.ctx());
    let origin = ui.max_rect().min;
    ui.painter().text(
        origin + vec2(18., 18.),
        egui::Align2::LEFT_TOP,
        "添加开发目录",
        egui::FontId::new(14., egui::FontFamily::Name("md-bold".into())),
        Color32::from_gray(51),
    );
    controls::label(ui, origin + vec2(18., 55.), "目录名称：");
    controls::label(ui, origin + vec2(18., 90.), "目录路径：");
    controls::label(ui, origin + vec2(18., 121.), "项目类型：");
    controls::field(
        ui,
        Rect::from_min_size(origin + vec2(88., 51.), vec2(328., 23.)),
        &mut draft.fields[0],
        "directory-name",
        &mut draft.project.name,
        "如：backend",
        true,
    );
    let path_changed = controls::field(
        ui,
        Rect::from_min_size(origin + vec2(88., 86.), vec2(280., 23.)),
        &mut draft.fields[1],
        "directory-path",
        &mut draft.project.path,
        "选择一个本地目录路径",
        true,
    );
    if path_changed {
        draft.changed();
    }
    if path_changed {
        draft.project.project_type = "GENERAL".into();
    }
    ui.painter().text(
        origin + vec2(88., 121.),
        egui::Align2::LEFT_TOP,
        project::type_label(&draft.project.project_type),
        egui::FontId::new(12., egui::FontFamily::Name("md-bold".into())),
        Color32::from_rgb(25, 118, 210),
    );
    ui.painter().text(
        origin + vec2(18., 150.),
        egui::Align2::LEFT_TOP,
        "提示：目录仅记录路径信息，默认不做任何自动发布动作。",
        controls::font(ui, 11.),
        Color32::from_gray(153),
    );
    let browse = controls::button(
        ui,
        Rect::from_min_size(origin + vec2(374., 86.), vec2(42., 24.)),
        "浏览",
        false,
        !busy,
    );
    let cancel = controls::button(
        ui,
        Rect::from_min_size(origin + vec2(352., 179.), vec2(40., 24.)),
        "取消",
        false,
        !busy,
    );
    let rect = Rect::from_min_size(origin + vec2(402., 179.), vec2(40., 24.));
    let accept = ui.interact(
        rect,
        ui.id().with("directory-accept"),
        if busy {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        },
    );
    accept.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, !busy, "确定"));
    ui.painter().rect_filled(
        rect,
        4,
        Color32::from_rgb(7, 193, 96).gamma_multiply(if busy { 0.4 } else { 1. }),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "确定",
        egui::FontId::new(12., egui::FontFamily::Name("md-bold".into())),
        Color32::WHITE,
    );
    if std::mem::take(&mut draft.focus) {
        draft.fields[0].select(ui.ctx(), 0..0);
    }
    if accept.clicked() && !busy {
        Some(Action::Save)
    } else if cancel.clicked() {
        Some(Action::Cancel)
    } else if browse.clicked() {
        Some(Action::Browse)
    } else {
        None
    }
}
pub(super) fn show(ctx: &egui::Context, state: &mut State, icons: &mut Icons) {
    let Some(mut draft) = state.add.take() else {
        return;
    };
    draft.context = Some(ctx.clone());
    let busy = state.pending.values().any(|r| {
        matches!(
            r,
            Request::Config { .. } | Request::Directory { save: true, .. }
        )
    });
    let mut action = None;
    let owner = draft
        .focus
        .then(|| ctx.input(|i| i.viewport().outer_rect))
        .flatten();
    let dismissed = super::native(
        ctx,
        "development-add",
        "添加目录",
        vec2(460., 220.),
        owner,
        |ui| {
            if busy || draft.alert.is_some() {
                ui.disable();
                ui.set_opacity(1.);
            }
            action = contents(ui, &mut draft, busy);
        },
    );
    let mut cancel = dismissed && !busy && draft.alert.is_none();
    if !busy
        && draft.alert.is_none()
        && let Some(action) = action
    {
        match action {
            Action::Cancel => cancel = true,
            Action::Save => {
                if draft.project.name.trim().is_empty() || draft.project.path.trim().is_empty() {
                    draft.warn("目录名称和路径不能为空");
                } else {
                    state.submit(
                        service::Action::InspectDirectory {
                            path: PathBuf::from(draft.project.path.trim()),
                        },
                        Request::Directory {
                            epoch: draft.epoch,
                            save: true,
                        },
                    );
                }
            }
            Action::Browse => {
                if let Some(path) = rfd::FileDialog::new().set_title("选择目录").pick_folder() {
                    draft.project.path = path.to_string_lossy().into_owned();
                    draft.fields[1].reset();
                    draft.changed();
                    state.submit(
                        service::Action::InspectDirectory { path },
                        Request::Directory {
                            epoch: draft.epoch,
                            save: false,
                        },
                    );
                }
            }
        }
    }
    if let Some(alert) = &draft.alert {
        let duplicate = matches!(alert, Alert::Duplicate(_));
        let content = match alert {
            Alert::Warning(text) => text.as_str(),
            Alert::Duplicate(_) => "目录名称已存在，仍要添加吗？",
        };
        let answer = message::show(
            ctx,
            Message {
                id: egui::ViewportId::from_hash_of("development-directory-warning"),
                owner: None,
                title: if duplicate { "确认" } else { "警告" },
                header: if duplicate { None } else { Some("警告") },
                content,
                kind: if duplicate {
                    Kind::Confirmation
                } else {
                    Kind::Warning
                },
                buttons: if duplicate {
                    &[
                        Button {
                            label: "是",
                            role: Role::Yes,
                        },
                        Button {
                            label: "否",
                            role: Role::No,
                        },
                    ]
                } else {
                    &[Button {
                        label: "确定",
                        role: Role::Ok,
                    }]
                },
                active: true,
            },
            &mut draft.focus_alert,
            icons,
            |_, _| {},
        );
        if let Some(answer) = answer {
            let alert = draft.alert.take();
            if matches!(answer, Answer::Button(0))
                && let Some(Alert::Duplicate(project)) = alert
            {
                persist(state, *project);
            }
        }
    }
    if !cancel {
        state.add = Some(draft);
    }
}

pub(super) fn preview(ui: &mut Ui, state: &mut State) {
    let draft = state.add.get_or_insert_with(Draft::new);
    draft.context = Some(ui.ctx().clone());
    ui.painter()
        .rect_filled(ui.max_rect(), 0, Color32::from_gray(244));
    let _ = contents(ui, draft, false);
}
