//! One replaceable terminal face; enumerate/load installed fonts off the UI thread.
use egui::{Context, FontFamily, FontId, Id};
use std::sync::{Arc, Mutex};
#[cfg(test)]
mod tests;
#[derive(Default)]
struct State {
    requested: Option<String>,
    names: Option<Arc<[String]>>,
    listing: bool,
    working: bool,
}
fn state(ctx: &Context, preview: bool) -> Arc<Mutex<State>> {
    ctx.data_mut(|data| {
        data.get_temp_mut_or_default::<Arc<Mutex<State>>>(Id::new(("terminal-font-state", preview)))
            .clone()
    })
}
pub(crate) fn font(ctx: &Context, size: f32) -> FontId {
    font_for(ctx, size, false)
}
fn font_for(ctx: &Context, size: f32, preview: bool) -> FontId {
    let family = FontFamily::Name(
        if preview {
            "ssh-preview"
        } else {
            "ssh-terminal"
        }
        .into(),
    );
    FontId::new(
        size,
        if ctx.fonts(|f| f.definitions().families.contains_key(&family)) {
            family
        } else {
            FontFamily::Monospace
        },
    )
}
pub(crate) fn preview(ctx: &Context, name: &str, size: f32) -> FontId {
    ensure_for(ctx, name, true);
    font_for(ctx, size, true)
}
pub(crate) fn ensure(ctx: &Context, name: &str) {
    ensure_for(ctx, name, false);
}
fn ensure_for(ctx: &Context, name: &str, preview: bool) {
    let family = FontFamily::Name(
        if preview {
            "ssh-preview"
        } else {
            "ssh-terminal"
        }
        .into(),
    );
    let slot = if preview {
        "terminal-preview-custom"
    } else {
        "terminal-custom"
    };
    let name = if name.trim().is_empty() {
        "monospace"
    } else {
        name.trim()
    }
    .to_owned();
    let state = state(ctx, preview);
    let mut current = match state.lock() {
        Ok(state) => state,
        Err(_) => return,
    };
    if current.requested.as_deref() == Some(&name) {
        return;
    }
    current.requested = Some(name.clone());
    if name.eq_ignore_ascii_case("monospace")
        || name.eq_ignore_ascii_case("consolas") && cfg!(windows)
    {
        super::update(ctx, |fonts| {
            fonts.font_data.remove(slot);
            fonts
                .families
                .insert(family, fonts.families[&FontFamily::Monospace].clone());
        });
        return;
    }
    if current.working {
        return;
    }
    current.working = true;
    drop(current);
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("terminal-font".into())
        .spawn(move || {
            use resvg::usvg::fontdb::{Database, Family, Query};
            let mut database = Database::new();
            database.load_system_fonts();
            loop {
                let name = match state.lock() {
                    Ok(current) => current.requested.clone().unwrap_or_default(),
                    Err(_) => return,
                };
                let face = database
                    .query(&Query {
                        families: &[Family::Name(&name)],
                        ..Default::default()
                    })
                    .and_then(|id| {
                        database.with_face_data(id, |bytes, index| {
                            let mut font = egui::FontData::from_owned(bytes.to_vec());
                            font.index = index;
                            Arc::new(font)
                        })
                    });
                // Holding this small lock also orders a late worker against a newer choice.
                let Ok(mut current) = state.lock() else {
                    return;
                };
                if current.requested.as_deref() != Some(&name) {
                    continue;
                }
                super::update(&ctx, |fonts| {
                    let mut fallback = fonts.families[&FontFamily::Monospace].clone();
                    fonts.font_data.remove(slot);
                    if let Some(face) = face {
                        fonts.font_data.insert(slot.into(), face);
                        fallback.insert(0, slot.into());
                    }
                    fonts.families.insert(family.clone(), fallback);
                });
                current.working = false;
                ctx.request_repaint();
                break;
            }
        });
}
pub(crate) fn names(ctx: &Context) -> Arc<[String]> {
    let shared = state(ctx, false);
    {
        let Ok(mut state) = shared.lock() else {
            return Arc::from([]);
        };
        if let Some(names) = &state.names {
            return names.clone();
        }
        if state.listing {
            return Arc::from([]);
        }
        state.listing = true;
    }
    let ctx = ctx.clone();
    let _ = std::thread::Builder::new()
        .name("terminal-font-list".into())
        .spawn(move || {
            let mut database = resvg::usvg::fontdb::Database::new();
            database.load_system_fonts();
            let mut names: Vec<_> = database
                .faces()
                .flat_map(|face| face.families.iter().map(|(name, _)| name))
                .filter(|name| {
                    let lower = name.to_lowercase();
                    [
                        "mono",
                        "consol",
                        "courier",
                        "menlo",
                        "dejavu",
                        "liberation",
                        "sarasa",
                        "cascadia",
                        "jetbrains",
                    ]
                    .iter()
                    .any(|part| lower.contains(part))
                })
                .cloned()
                .collect();
            names.sort();
            names.dedup();
            if let Ok(mut state) = shared.lock() {
                state.names = Some(names.into());
            }
            ctx.request_repaint();
        });
    Arc::from([])
}
