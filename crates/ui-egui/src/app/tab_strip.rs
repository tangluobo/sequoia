//! JavaFX connect-tree.css header geometry, independent of the tab's controller.
#[cfg(test)]
mod tests;
use super::*;
use std::borrow::Cow;

pub(super) struct Header {
    pub response: egui::Response,
    pub close: bool,
}

fn font() -> egui::FontId {
    egui::FontId::new(12., egui::FontFamily::Name("java-ui".into()))
}

fn title(tab: &Tab) -> Cow<'_, str> {
    match &tab.panel {
        Panel::Sql { workbench, .. } if workbench.designer.is_some() => {
            if workbench
                .designer
                .as_ref()
                .is_some_and(|d| d.dirty || d.edit.is_some())
            {
                format!("*{}", tab.title).into()
            } else {
                tab.title.as_str().into()
            }
        }
        Panel::Sql { workbench, .. } if workbench.document.is_some() => {
            workbench.document.as_ref().unwrap().title().into()
        }
        _ if tab.dirty() => format!("{} *", tab.title).into(),
        _ => tab.title.as_str().into(),
    }
}

// JavaFX RadioMenuItem parses mnemonic markers; the main Tab label does not.
// Match OpenJFX MnemonicInfo (including escapes and _(x)) without renaming a tab.
fn menu_label(text: &str) -> Cow<'_, str> {
    if !text.contains('_') {
        return text.into();
    }
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        if ch == '_'
            && let Some(&(_, next)) = chars.peek()
        {
            if next == '_' {
                result.push('_');
                chars.next();
                continue;
            }
            let rest = &text[at + 1..];
            if next == '(' {
                let mut extended = rest.chars();
                extended.next();
                if let (Some(key), Some(')')) = (extended.next(), extended.next())
                    && key.len_utf16() == 1
                    && !tomato_core::java_text::is_whitespace(key)
                {
                    result.push_str(&text[at + 3 + key.len_utf8()..]);
                    return result.into();
                }
            }
            if !tomato_core::java_text::is_whitespace(next) {
                result.push_str(rest);
                return result.into();
            }
        }
        result.push(ch);
    }
    result.into()
}

pub(super) fn width(ui: &Ui, tab: &Tab) -> f32 {
    let text = ui
        .painter()
        .layout_no_wrap(title(tab).into_owned(), font(), Color32::BLACK);
    25. + text.size().x.ceil()
        + if super::shell::tab_icon(tab).is_some() {
            22.
        } else {
            0.
        }
        + if matches!(&tab.panel,Panel::Sql{workbench,..}if workbench.browser.is_some()) {
            0.
        } else {
            16.
        }
}

impl TomatoApp {
    pub(super) fn tab_overflow(&mut self, ui: &mut Ui, rect: egui::Rect) {
        ui.painter()
            .rect_filled(rect, 0, crate::theme::NAV_BACKGROUND);
        self.icons.paint(
            ui,
            "images/tab-overflow.svg",
            egui::Rect::from_min_size(rect.min + egui::vec2(0., 13.), egui::vec2(19., 19.)),
        );
        let response = ui.interact(
            egui::Rect::from_min_max(rect.min + egui::vec2(0., 14.), rect.max),
            ui.make_persistent_id("tab-overflow"),
            egui::Sense::click(),
        );
        let frame = egui::Frame::popup(ui.style())
            .fill(Color32::WHITE)
            .stroke(egui::Stroke::new(1., Color32::from_gray(192)))
            .corner_radius(0)
            .inner_margin(egui::Margin::symmetric(0, 3));
        let mut popup = egui::Popup::menu(&response).frame(frame);
        let fixture = std::env::var("TOMATO_UI_PREVIEW").as_deref() == Ok("tab_strip")
            && std::env::var("TOMATO_TAB_STATE").as_deref() == Ok("popup")
            && std::env::var_os("TOMATO_UI_CAPTURE").is_some();
        if fixture {
            popup = popup.open(true);
        }
        popup.show(|ui| {
            ui.spacing_mut().item_spacing.y=0.;
            let width=self.tabs.iter().filter(|tab|tab.workspace_visible()).map(|tab|ui.painter().layout_no_wrap(menu_label(&title(tab)).into_owned(),font(),Color32::BLACK).size().x.ceil()).fold(0.,f32::max)+32.;
            let mut rows=Vec::new();
            for tab in self.tabs.iter().filter(|tab| tab.workspace_visible()) {
                let caption=title(tab);let label=menu_label(&caption);let selected=self.active.as_deref()==Some(tab.id.as_str());
                let response=menu_row(ui,&label,selected,width);
                if fixture {rows.push(serde_json::json!({"title":caption,"label":label,"selected":selected,"x":response.rect.left(),"y":response.rect.top(),"width":response.rect.width(),"height":response.rect.height()}));}
                if response.clicked() {
                    self.active = Some(tab.id.clone());
                    ui.close();
                }
            }
            if fixture&&let Some(path)=std::env::var_os("TOMATO_UI_CAPTURE").map(PathBuf::from){let _=std::fs::write(path.with_extension("popup.json"),serde_json::json!({"rows":rows,"networkRequested":false,"rowHeight":crate::compact_menu::ROW_HEIGHT}).to_string());}
        });
    }
}

fn menu_row(ui: &mut Ui, label: &str, selected: bool, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, crate::compact_menu::ROW_HEIGHT),
        egui::Sense::click(),
    );
    let hover = response.hovered() || response.has_focus();
    if hover {
        ui.painter()
            .rect_filled(rect, 0, Color32::from_rgb(0, 150, 201));
    }
    let color = if hover {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let text = ui.painter().layout_no_wrap(label.into(), font(), color);
    ui.painter().galley(
        egui::pos2(
            rect.left() + 20.,
            rect.top() + ((rect.height() - text.size().y) * 0.5).round(),
        ),
        text,
        color,
    );
    if selected {
        let points = vec![
            rect.min + egui::vec2(6., 11.),
            rect.min + egui::vec2(9., 15.),
            rect.min + egui::vec2(15., 7.),
        ];
        ui.painter().add(egui::Shape::line(
            points,
            egui::Stroke::new(
                1.7,
                if hover {
                    Color32::WHITE
                } else {
                    Color32::from_gray(85)
                },
            ),
        ));
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    response
}

pub(super) fn header(
    ui: &mut Ui,
    tab: &Tab,
    icons: &mut crate::icons::Icons,
    selected: bool,
) -> Header {
    let icon = super::shell::tab_icon(tab);
    let closable = !matches!(&tab.panel, Panel::Sql {workbench,..} if workbench.browser.is_some());
    let text = title(tab);
    let color = Color32::from_gray(if selected { 0 } else { 85 });
    let galley = ui
        .painter()
        .layout_no_wrap(text.into_owned(), font(), color);
    // JavaFX rounds the label's preferred width up. The tab includes 12 px on
    // either side, a 1 px right border, graphic/text gap 4 and close slot 16.
    let label_width = galley.size().x.ceil() + if icon.is_some() { 22. } else { 0. };
    let width = 25. + label_width + if closable { 16. } else { 0. };
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, crate::theme::TAB_HEIGHT),
        egui::Sense::click(),
    );
    let painter = ui.painter();
    // The original base and selected backgrounds are !important. Hover/focus
    // therefore retain these colors, including when hovering the close shape.
    painter.rect_filled(
        rect,
        0,
        if selected {
            Color32::from_rgb(252, 255, 255)
        } else {
            Color32::from_gray(224)
        },
    );
    painter.rect_filled(
        egui::Rect::from_min_max(egui::pos2(rect.right() - 1., rect.top()), rect.max),
        0,
        Color32::from_gray(192),
    );
    painter.rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.bottom() - 1.),
            egui::pos2(rect.right() - 1., rect.bottom()),
        ),
        0,
        if selected {
            Color32::BLACK
        } else {
            Color32::from_gray(192)
        },
    );
    if let Some(icon) = icon {
        icons.paint(
            ui,
            icon,
            egui::Rect::from_min_size(rect.min + egui::vec2(12., 9.), egui::vec2(18., 18.)),
        );
    }
    ui.painter().galley(
        egui::pos2(
            rect.left() + 12. + if icon.is_some() { 22. } else { 0. },
            rect.top() + ((36. - galley.size().y) * 0.5).round(),
        ),
        galley,
        color,
    );
    let mut close = false;
    if closable {
        let slot = egui::Rect::from_min_size(
            rect.min + egui::vec2(12. + label_width, 10.),
            egui::vec2(16., 16.),
        );
        let center = slot.center();
        for sign in [-1., 1.] {
            ui.painter().line_segment(
                [
                    center + egui::vec2(-4., -4. * sign),
                    center + egui::vec2(4., 4. * sign),
                ],
                egui::Stroke::new(1., Color32::from_gray(153)),
            );
        }
        close = ui
            .interact(
                slot,
                ui.make_persistent_id(("close", &tab.id)),
                egui::Sense::click(),
            )
            .clicked();
    }
    Header { response, close }
}
