//! A replaceable table face and a lazily enumerated family list, not a font cache per tab.
use egui::{Context, FontFamily, Id};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct State {
    requested: String,
    working: bool,
}
fn state(ctx: &Context) -> Arc<Mutex<State>> {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_default::<Arc<Mutex<State>>>(Id::new("table-font-loader"))
            .clone()
    })
}
pub(crate) fn default_face(name: &str) -> bool {
    ["Sans Serif", "sans-serif", "System", "Dialog"]
        .iter()
        .any(|v| name.eq_ignore_ascii_case(v))
}
pub(crate) fn ensure(ctx: &Context, name: &str) {
    let shared = state(ctx);
    let Ok(mut current) = shared.lock() else {
        return;
    };
    if current.requested == name {
        return;
    }
    current.requested = name.to_owned();
    if default_face(name) {
        super::update(ctx, |fonts| {
            for key in ["table-user", "table-user-bold"] {
                fonts.font_data.remove(key);
                fonts.families.remove(&FontFamily::Name(key.into()));
            }
        });
        return;
    }
    if current.working {
        return;
    }
    current.working = true;
    drop(current);
    let ctx = ctx.clone();
    let retained = shared.clone();
    let result = std::thread::Builder::new()
        .name("table-font".into())
        .spawn(move || {
            use resvg::usvg::fontdb::{Database, Family, Query, Weight};
            let mut database = Database::new();
            database.load_system_fonts();
            loop {
                let name = match shared.lock() {
                    Ok(s) => s.requested.clone(),
                    Err(_) => return,
                };
                if default_face(&name) {
                    if let Ok(mut s) = shared.lock() {
                        s.working = false;
                    }
                    return;
                }
                let family = match name.as_str() {
                    "Serif" => Family::Serif,
                    "Monospace" | "DialogInput" => Family::Monospace,
                    _ => Family::Name(&name),
                };
                let regular = database.query(&Query {
                    families: &[family],
                    ..Default::default()
                });
                let bold = database.query(&Query {
                    families: &[family],
                    weight: Weight::BOLD,
                    ..Default::default()
                });
                let load = |id| {
                    database.with_face_data(id, |bytes, index| {
                        let mut face = egui::FontData::from_owned(bytes.to_vec());
                        face.index = index;
                        Arc::new(face)
                    })
                };
                let normal = regular.and_then(load);
                let bold = if bold == regular {
                    normal.clone()
                } else {
                    bold.and_then(load).or_else(|| normal.clone())
                };
                let Ok(mut s) = shared.lock() else { return };
                if s.requested != name {
                    continue;
                }
                super::update(&ctx, |fonts| {
                    for (slot, source, face) in [
                        ("table-user", "table", normal),
                        ("table-user-bold", "table-bold", bold),
                    ] {
                        let mut fallback = fonts
                            .families
                            .get(&FontFamily::Name(source.into()))
                            .cloned()
                            .unwrap_or_else(|| fonts.families[&FontFamily::Proportional].clone());
                        fonts.font_data.remove(slot);
                        if let Some(face) = face {
                            fonts.font_data.insert(slot.into(), face);
                            fallback.insert(0, slot.into());
                        }
                        fonts
                            .families
                            .insert(FontFamily::Name(slot.into()), fallback);
                    }
                });
                s.working = false;
                ctx.request_repaint();
                break;
            }
        });
    if result.is_err()
        && let Ok(mut state) = retained.lock()
    {
        state.working = false;
        state.requested.clear();
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn replacing_a_table_font_releases_the_previous_byte_owner() {
        let ctx = Context::default();
        let frame = || {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                ui.label("font cache");
            });
            out.textures_delta.clear();
        };
        frame();
        ensure(&ctx, "Arial");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        let weak = loop {
            frame();
            if let Some(weak) = ctx.fonts(|fonts| {
                fonts
                    .definitions()
                    .font_data
                    .get("table-user")
                    .map(Arc::downgrade)
            }) {
                break weak;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "table font worker did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        ensure(&ctx, "Sans Serif");
        frame();
        frame();
        assert!(ctx.fonts(|fonts| !fonts.definitions().font_data.contains_key("table-user")));
        drop(ctx);
        assert!(
            weak.upgrade().is_none(),
            "replaced table font bytes remained retained"
        );
    }
}
