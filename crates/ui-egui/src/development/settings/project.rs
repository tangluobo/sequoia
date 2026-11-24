use super::*;
use egui::{Color32, Rect};
use tomato_core::development::runtime::{self, EntryRole};
pub(super) enum Action {
    Save,
    Cancel,
    Manager(manager::Action),
}
pub(super) fn contents(ui: &mut Ui, draft: &mut Draft, busy: bool) -> Option<Action> {
    crate::panels::ensure_bold_font(ui.ctx());
    controls::tabs(ui, &mut draft.tab);
    let mut action = None;
    if draft.tab == 0 {
        let origin = ui.max_rect().min;
        let p = &mut draft.config.projects[draft.index];
        controls::label(ui, origin + vec2(30., 59.), "项目：");
        controls::label(ui, origin + vec2(102., 59.), &p.name);
        controls::label(ui, origin + vec2(30., 88.), "识别类型：");
        ui.painter().text(
            origin + vec2(102., 88.),
            egui::Align2::LEFT_TOP,
            type_label(&p.project_type),
            egui::FontId::new(12., egui::FontFamily::Name("md-bold".into())),
            Color32::from_rgb(25, 118, 210),
        );
        for (row, (label, kind, selected)) in [
            ("JDK：", "JDK", &mut p.jdk_id),
            ("Node：", "NODE", &mut p.node_id),
            ("Flutter：", "FLUTTER", &mut p.flutter_id),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 117. + row as f32 * 37.;
            controls::label(ui, origin + vec2(30., y + 4.), label);
            let values = draft
                .config
                .runtimes
                .iter()
                .filter(|v| v.kind == kind)
                .map(|v| {
                    (
                        v.id.clone(),
                        format!("{} ({})", v.name, runtime::known_version(&v.version)),
                    )
                })
                .collect::<Vec<_>>();
            controls::combo(
                ui,
                Rect::from_min_size(origin + vec2(102., y), vec2(360., 23.)),
                kind,
                selected,
                &format!("未指定 {}", runtime::display_kind(kind)),
                &values,
                true,
            );
        }
        controls::label(ui, origin + vec2(30., 232.), "构建工具：");
        let kind = runtime::build_kind(&p.project_type);
        let values = draft
            .config
            .build_tools
            .iter()
            .filter(|v| Some(v.kind.as_str()) == kind)
            .map(|v| {
                (
                    v.id.clone(),
                    format!("{} ({})", v.name, runtime::known_version(&v.version)),
                )
            })
            .collect::<Vec<_>>();
        let prompt = kind.map_or_else(
            || "当前项目未识别构建工具".into(),
            |kind| format!("未指定 {}", runtime::display_kind(kind)),
        );
        controls::combo(
            ui,
            Rect::from_min_size(origin + vec2(102., 228.), vec2(360., 23.)),
            "build-tool",
            &mut p.maven_id,
            &prompt,
            &values,
            kind.is_some(),
        );
    } else if let Some(value) = manager::show(
        ui,
        draft,
        if draft.tab == 2 {
            EntryRole::BuildTool
        } else {
            EntryRole::Runtime
        },
    ) {
        action = Some(Action::Manager(value));
    }
    let (save, cancel) = controls::footer(ui, "确定", !busy, &mut draft.focus_main);
    if save {
        action = Some(Action::Save);
    }
    if cancel {
        action = Some(Action::Cancel);
    }
    action
}
pub(super) fn dialogs(ctx: &egui::Context, state: &mut State, busy: bool, icons: &mut Icons) {
    let Some(mut draft) = state.settings.take() else {
        return;
    };
    draft.bind(ctx);
    let mut action = None;
    let title = format!("项目配置 - {}", draft.config.projects[draft.index].name);
    let center = draft
        .focus_main
        .then(|| ctx.input(|i| i.viewport().outer_rect))
        .flatten();
    let dismissed = super::native(
        ctx,
        "development-settings",
        &title,
        vec2(660., 503.),
        center,
        |ui| {
            if draft.edit.is_some() || busy {
                ui.disable();
                ui.set_opacity(1.);
            }
            action = contents(ui, &mut draft, busy);
        },
    );
    let mut cancel = dismissed && !busy && draft.edit.is_none();
    if draft.edit.is_none()
        && !busy
        && let Some(action) = action
    {
        match action {
            Action::Save => state.save_config(
                draft.config.clone(),
                state.project().map(|p| p.path.clone()),
                None,
            ),
            Action::Cancel => cancel = true,
            Action::Manager(manager::Action::Add(role)) => draft.begin(role, None),
            Action::Manager(manager::Action::Edit(index)) => {
                draft.begin(EntryRole::BuildTool, Some(index))
            }
            Action::Manager(manager::Action::Remove(role)) => draft.remove(role),
        }
    }
    entry::dialogs(ctx, state, &mut draft, icons);
    if !cancel {
        state.settings = Some(draft);
    }
}

pub(super) fn type_label(kind: &str) -> &str {
    match kind {
        "MAVEN" => "Maven 项目",
        "NODE" => "Node 项目",
        "MAVEN_NODE" => "Maven + Node 项目",
        "GRADLE" => "Gradle 项目",
        "GRADLE_NODE" => "Gradle + Node 项目",
        "FLUTTER" => "Flutter 项目",
        _ => "普通目录",
    }
}
