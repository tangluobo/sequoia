use super::*;
use egui::Rect;
pub(super) enum Action {
    Accept,
    Cancel,
    BrowseHome,
    BrowseSettings,
    TypeChanged,
    PathChanged,
    VersionChanged,
    NameChanged,
}
pub(super) fn contents(ui: &mut Ui, draft: &mut Draft, busy: bool) -> Option<Action> {
    let edit = draft.edit?;
    let build = edit.role.is_build();
    let origin = ui.max_rect().min;
    let x = if build { 107. } else { 62. };
    let width = if build { 450. } else { 430. };
    let mut action = None;
    controls::label(ui, origin + vec2(16., 20.), "类型：");
    let types = if build {
        &["MAVEN", "GRADLE"][..]
    } else {
        &["JDK", "NODE", "FLUTTER"][..]
    };
    let values = types
        .iter()
        .map(|v| ((*v).into(), (*v).into()))
        .collect::<Vec<_>>();
    if controls::combo(
        ui,
        Rect::from_min_size(
            origin + vec2(x, 16.),
            vec2(if build { 94. } else { 96. }, 23.),
        ),
        "entry-kind",
        &mut draft.kind,
        "",
        &values,
        true,
    ) {
        action = Some(Action::TypeChanged);
    }
    controls::label(ui, origin + vec2(16., 55.), "名称：");
    if controls::field(
        ui,
        Rect::from_min_size(origin + vec2(x, 51.), vec2(width, 23.)),
        &mut draft.fields[0],
        "entry-name",
        &mut draft.name,
        if build {
            "如：Maven 3.9"
        } else {
            "SDK 名称"
        },
        true,
    ) {
        action = Some(Action::NameChanged);
    }
    controls::label(
        ui,
        origin + vec2(16., 90.),
        if build {
            "工具路径："
        } else {
            "路径："
        },
    );
    if controls::field(
        ui,
        Rect::from_min_size(origin + vec2(x, 86.), vec2(width - 48., 23.)),
        &mut draft.fields[1],
        "entry-path",
        &mut draft.path,
        if build {
            "构建工具主目录"
        } else {
            "SDK 安装路径"
        },
        true,
    ) {
        action = Some(Action::PathChanged);
    }
    if controls::button(
        ui,
        Rect::from_min_size(origin + vec2(x + width - 40., 86.), vec2(40., 24.)),
        "浏览",
        false,
        true,
    )
    .clicked()
    {
        action = Some(Action::BrowseHome);
    }
    controls::label(ui, origin + vec2(16., 125.), "版本：");
    if controls::field(
        ui,
        Rect::from_min_size(origin + vec2(x, 121.), vec2(width, 23.)),
        &mut draft.fields[2],
        "entry-version",
        &mut draft.version,
        "自动识别，也可以手动填写",
        true,
    ) {
        action = Some(Action::VersionChanged);
    }
    if build {
        let maven = draft.kind == "MAVEN";
        controls::label(ui, origin + vec2(16., 160.), "settings.xml：");
        controls::field(
            ui,
            Rect::from_min_size(origin + vec2(x, 156.), vec2(width - 48., 23.)),
            &mut draft.fields[3],
            "entry-settings",
            &mut draft.settings,
            "默认使用 Maven/conf/settings.xml",
            maven,
        );
        ui.push_id("settings-picker", |ui| {
            if controls::button(
                ui,
                Rect::from_min_size(origin + vec2(x + width - 40., 156.), vec2(40., 24.)),
                "浏览",
                false,
                maven,
            )
            .clicked()
            {
                action = Some(Action::BrowseSettings);
            }
        });
    }
    let (accept, cancel) = controls::footer(
        ui,
        if edit.index.is_some() {
            "保存"
        } else {
            "添加"
        },
        !busy,
        &mut draft.focus_entry,
    );
    if accept {
        action = Some(Action::Accept);
    }
    if cancel {
        action = Some(Action::Cancel);
    }
    action
}
pub(super) fn dialogs(
    ctx: &egui::Context,
    state: &mut State,
    draft: &mut Draft,
    icons: &mut Icons,
) {
    let Some(edit) = draft.edit else { return };
    let busy = state
        .pending
        .values()
        .any(|request| matches!(request,Request::ValidateEntry{epoch} if *epoch==edit.epoch));
    let detecting = state
        .pending
        .values()
        .any(|request| matches!(request,Request::Detect{epoch,..} if *epoch==edit.epoch));
    let title = if edit.role.is_build() {
        if edit.index.is_some() {
            "修改构建工具"
        } else {
            "添加构建工具"
        }
    } else {
        "添加 SDK"
    };
    let size = if edit.role.is_build() {
        vec2(573., 238.)
    } else {
        vec2(508., 203.)
    };
    let center = draft
        .focus_entry
        .then(|| ctx.input(|i| i.viewport().outer_rect))
        .flatten();
    let mut action = None;
    let dismissed = super::native(ctx, "development-runtime-edit", title, size, center, |ui| {
        if busy || draft.warning.is_some() {
            ui.disable();
            ui.set_opacity(1.);
        }
        action = contents(ui, draft, busy || detecting);
    });
    if dismissed && !busy && draft.warning.is_none() {
        action = Some(Action::Cancel);
    }
    let mut detect = false;
    if !busy
        && draft.warning.is_none()
        && let Some(action) = action
    {
        match action {
            Action::Accept => {
                state.submit(
                    service::Action::ValidateEntry {
                        role: edit.role,
                        kind: draft.kind.clone(),
                        name: draft.name.clone(),
                        path: PathBuf::from(draft.path.trim()),
                        settings: PathBuf::from(draft.settings.trim()),
                    },
                    Request::ValidateEntry { epoch: edit.epoch },
                );
            }
            Action::Cancel => draft.close_entry(),
            Action::TypeChanged => draft.change_kind(),
            Action::PathChanged | Action::VersionChanged => draft.invalidate_detection(),
            Action::NameChanged => draft.name_changed(),
            Action::BrowseHome => {
                let path = if draft.kind == "NODE" && !edit.role.is_build() {
                    rfd::FileDialog::new()
                        .set_title("选择 Node 可执行文件")
                        .add_filter("Node 可执行文件", &["exe", "*"])
                        .pick_file()
                } else {
                    rfd::FileDialog::new()
                        .set_title(format!("选择 {} 主目录", draft.kind))
                        .pick_folder()
                };
                if let Some(path) = path {
                    draft.browsed(path);
                    detect = true;
                }
            }
            Action::BrowseSettings => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("选择 Maven settings.xml")
                    .add_filter("Maven settings.xml", &["xml"])
                    .pick_file()
                {
                    draft.settings = path.to_string_lossy().into_owned();
                }
            }
        }
    }
    if detect && let Some(edit) = draft.edit {
        state.submit(
            service::Action::DetectVersion {
                kind: draft.kind.clone(),
                path: PathBuf::from(&draft.path),
            },
            Request::Detect {
                kind: draft.kind.clone(),
                path: PathBuf::from(&draft.path),
                epoch: edit.epoch,
            },
        );
    }
    if let Some(warning) = draft.warning.clone() {
        use crate::native_dialog::message::{self, Button, Kind, Message, Role};
        let answer = message::show(
            ctx,
            Message {
                id: egui::ViewportId::from_hash_of("development-sdk-warning"),
                owner: None,
                title: "警告",
                header: Some("警告"),
                content: &warning,
                kind: Kind::Warning,
                buttons: &[Button {
                    label: "确定",
                    role: Role::Ok,
                }],
                active: true,
            },
            &mut draft.warning_focus,
            icons,
            |_, _| {},
        );
        if answer.is_some() {
            draft.warning = None;
        }
    }
}
