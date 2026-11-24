//! Controls measured from SqlEditorView; connection data is borrowed from the store.
use crate::tabs::sql_scope::Selectors;
use egui::*;
use tomato_core::Connection;
pub(super) enum Action {
    Save,
    Format,
    Ai,
    Settings,
    Run,
    Explain,
    Connection(String),
    Database(String),
}
fn button(
    ui: &mut Ui,
    icons: &mut crate::icons::Icons,
    rect: Rect,
    id: Id,
    text: &str,
    icon: &str,
    enabled: bool,
) -> Response {
    let response = ui.interact(
        rect,
        id,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let fill = if enabled && response.is_pointer_button_down_on() {
        Color32::from_rgb(192, 220, 243)
    } else if enabled && response.hovered() {
        Color32::from_rgb(216, 230, 242)
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter().rect(
            rect,
            2.,
            fill,
            Stroke::new(
                1.,
                if response.is_pointer_button_down_on() {
                    Color32::from_rgb(144, 200, 246)
                } else {
                    Color32::from_gray(204)
                },
            ),
            StrokeKind::Inside,
        );
    }
    if let Some(texture) = icons.get(ui.ctx(), icon) {
        ui.painter().image(
            texture,
            Rect::from_min_size(rect.min + vec2(8., 5.), vec2(16., 16.)),
            Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
            if enabled {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(102)
            },
        );
    }
    ui.painter().text(
        rect.min + vec2(28., 6.),
        Align2::LEFT_TOP,
        text,
        FontId::proportional(12.),
        if enabled {
            Color32::from_rgb(51, 51, 51)
        } else {
            Color32::from_gray(153)
        },
    );
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, text));
    response
}
struct Field<'a> {
    rect: Rect,
    id: Id,
    icon: Option<&'a str>,
}
fn selector(
    ui: &mut Ui,
    text: &mut String,
    editor: &mut crate::tabs::text_state::Editor,
    items: impl Iterator<Item = (String, String)>,
    icons: &mut crate::icons::Icons,
    field: Field<'_>,
) -> Option<String> {
    let Field { rect, id, icon } = field;
    ui.painter().rect(
        rect,
        0.,
        Color32::WHITE,
        Stroke::new(1., Color32::from_gray(185)),
        StrokeKind::Inside,
    );
    let arrow = Rect::from_min_max(pos2(rect.right() - 22., rect.top()), rect.max);
    let response = ui.interact(arrow, id.with("arrow"), Sense::CLICK);
    let center = arrow.center();
    ui.painter().add(Shape::convex_polygon(
        vec![
            center + vec2(-3.5, -1.5),
            center + vec2(3.5, -1.5),
            center + vec2(0., 2.5),
        ],
        Color32::from_gray(70),
        Stroke::NONE,
    ));
    let mut left = rect.left() + 4.;
    if let Some(icon) = icon
        && let Some(texture) = icons.get(ui.ctx(), icon)
    {
        ui.painter().image(
            texture,
            Rect::from_min_size(pos2(left, rect.top() + 6.), vec2(16., 16.)),
            Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
            Color32::WHITE,
        );
        left += 20.;
    }
    let field = Rect::from_min_max(
        pos2(left, rect.top() + 3.),
        pos2(arrow.left(), rect.bottom() - 3.),
    );
    let id = editor.prepare_id(ui.ctx(), id);
    let output = ui
        .scope_builder(UiBuilder::new().max_rect(field), |ui| {
            ui.add(
                TextEdit::singleline(text)
                    .id(id)
                    .font(FontId::proportional(12.))
                    .frame(Frame::NONE.inner_margin(Margin::symmetric(0, 2)))
                    .desired_width(field.width()),
            )
        })
        .inner;
    let mut chosen = None;
    if output.lost_focus() || (output.has_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
        chosen = Some(text.trim_matches(|c: char| c <= ' ').to_owned());
    }
    Popup::menu(&response).width(rect.width()).show(|ui| {
        ScrollArea::vertical().max_height(300.).show(ui, |ui| {
            for (label, item_icon) in items {
                let (row, response) =
                    ui.allocate_exact_size(vec2(rect.width() - 8., 26.), Sense::click());
                if response.hovered() {
                    ui.painter()
                        .rect_filled(row, 0., Color32::from_rgb(216, 230, 242));
                }
                if let Some(texture) = icons.get(ui.ctx(), &item_icon) {
                    ui.painter().image(
                        texture,
                        Rect::from_min_size(row.min + vec2(2., 5.), vec2(16., 16.)),
                        Rect::from_min_max(Pos2::ZERO, pos2(1., 1.)),
                        Color32::WHITE,
                    );
                }
                ui.painter().text(
                    row.min + vec2(22., 6.),
                    Align2::LEFT_TOP,
                    &label,
                    FontId::proportional(12.),
                    Color32::from_gray(51),
                );
                if response.clicked() {
                    chosen = Some(label);
                    ui.close();
                }
            }
        });
    });
    editor.trim(ui.ctx());
    chosen
}
pub(super) struct Context<'a> {
    pub connection: &'a Connection,
    pub namespace: &'a str,
    pub catalog: &'a [Connection],
    pub databases: &'a [String],
    pub ai_pending: bool,
    pub editable: bool,
}
pub(super) fn render(
    ui: &mut Ui,
    state: &mut Selectors,
    icons: &mut crate::icons::Icons,
    context: Context<'_>,
) -> Option<Action> {
    let Context {
        connection,
        namespace,
        catalog,
        databases,
        ai_pending,
        editable,
    } = context;
    let database = crate::tabs::workbench::query_database(connection, namespace);
    if state
        .identity
        .as_ref()
        .is_none_or(|(id, db)| id != &connection.id || db != database)
    {
        state.connection = connection.name.clone();
        state.database = database.into();
        state.identity = Some((connection.id.clone(), database.into()));
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 37.), Sense::hover());
    ui.painter().rect_filled(rect, 0., Color32::from_gray(248));
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1., Color32::from_gray(224)),
    );
    let at = |x, y, w, h| Rect::from_min_size(rect.min + vec2(x, y), vec2(w, h));
    let mut action = None;
    let connection_icon = crate::icons::for_connection(connection);
    if let Some(name) = selector(
        ui,
        &mut state.connection,
        &mut state.editors[0],
        catalog
            .iter()
            .filter(|c| {
                matches!(
                    c.kind(),
                    "MYSQL" | "POSTGRESQL" | "SQLITE" | "ORACLE" | "HIVE"
                )
            })
            .map(|c| (c.name.clone(), crate::icons::for_connection(c))),
        icons,
        Field {
            rect: at(8., 4., 140., 28.),
            id: ui.id().with("sql-connection"),
            icon: Some(&connection_icon),
        },
    ) && name != connection.name
    {
        action = Some(Action::Connection(name));
    }
    if let Some(name) = selector(
        ui,
        &mut state.database,
        &mut state.editors[1],
        databases
            .iter()
            .map(|db| (db.clone(), "images/connect/database.png".into())),
        icons,
        Field {
            rect: at(154., 4., 120., 28.),
            id: ui.id().with("sql-database"),
            icon: Some("images/connect/database.png"),
        },
    ) && name != database
    {
        action = Some(Action::Database(name));
    }
    ui.painter().vline(
        rect.left() + 283.,
        rect.top() + 4. ..=rect.top() + 32.,
        Stroke::new(1., Color32::from_gray(211)),
    );
    let extra = if ai_pending { 22. } else { 0. };
    for (index, (x, width, text, icon, kind, enabled)) in [
        (292.0_f32, 62., "保存", "save", Action::Save, true),
        (
            360.,
            110.,
            "创建查询工具",
            "create_query_tool",
            Action::Format,
            editable,
        ),
        (476., 62., "美化", "beautiful", Action::Format, editable),
        (
            544.,
            76. + extra,
            if ai_pending {
                "AI 生成中..."
            } else {
                "AI SQL"
            },
            "code",
            Action::Ai,
            !ai_pending,
        ),
        (646. + extra, 62., "运行", "execute", Action::Run, true),
        (714. + extra, 62., "解释", "code", Action::Explain, true),
    ]
    .into_iter()
    .enumerate()
    {
        if button(
            ui,
            icons,
            at(x, 5., width, 26.),
            ui.id().with(("sql-toolbar", index)),
            text,
            &format!("images/connect/{icon}.png"),
            enabled,
        )
        .clicked()
        {
            action = Some(kind);
        }
    }
    let arrow = ui.interact(
        at(620. + extra, 5., 20., 26.),
        ui.id().with("sql-ai-menu"),
        if ai_pending {
            Sense::hover()
        } else {
            Sense::click()
        },
    );
    let center = arrow.rect.center();
    ui.painter().add(Shape::convex_polygon(
        vec![
            center + vec2(-3., -1.),
            center + vec2(3., -1.),
            center + vec2(0., 2.),
        ],
        Color32::from_gray(51),
        Stroke::NONE,
    ));
    Popup::menu(&arrow).show(|ui| {
        if ui.button("生成 SQL（Ctrl+I）").clicked() {
            action = Some(Action::Ai);
            ui.close();
        }
        if ui.button("AI SQL 设置...").clicked() {
            action = Some(Action::Settings);
            ui.close();
        }
    });
    ui.advance_cursor_after_rect(rect);
    action
}
