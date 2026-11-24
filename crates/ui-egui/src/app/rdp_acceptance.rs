//! Opt-in metadata for the explicitly authorized saved-host acceptance run.
use super::*;

pub(super) fn saved_target() -> Option<(&'static str, &'static str)> {
    match (
        std::env::var("TOMATO_TEST_SAVED_110").as_deref() == Ok("authorized"),
        std::env::var("TOMATO_TEST_SAVED_LINUX").as_deref() == Ok("authorized"),
    ) {
        (true, false) => Some(("902daa67-8132-4308-b81b-27f9b8a05a3a", "110")),
        (false, true) => Some(("d0c01833-407d-484e-a407-fc4632aabd5c", "小主机桌面")),
        _ => None,
    }
}

impl TomatoApp {
    pub fn authorized_rdp_diagnostics(&self) -> Option<serde_json::Value> {
        if let Some(shutdown) = &self.shutdown {
            return shutdown.rdp_diagnostics.clone();
        }
        let (source_id, name) = saved_target()?;
        let sessions: Vec<_> = self.tabs.iter().filter_map(|tab| {
            if tab.source_id != source_id { return None; }
            let Panel::NativeDesktop { session: Some(session), view, .. } = &tab.panel else { return None; };
            let input = session.input_dispatch_stats().unwrap_or_default();
            let (revision, pointer) = session.pointer();
            Some(serde_json::json!({
                "mode": if view.window.is_some() { "WINDOW" } else { "TAB" },
                "ready": session.status() == tomato_services::remote_desktop::Status::Ready,
                "canvas": view.size,
                "texturePresent": view.texture_id().is_some(),
                "pointerRevision": revision,
                "pointerSize": pointer.as_ref().map(|p|p.size),
                "pointerHidden": session.pointer_hidden(),
                "nativePointerImageCached": view.native_pointer.is_some(),
                "inputMessages": input.messages,
                "meanInputDispatchMicros": input.mean_micros,
                "maxInputDispatchMicros": input.max_micros,
                "dispatchScope": "GUI service queue to TLS write completion; excludes server processing and physical display latency"
            }))
        }).collect();
        Some(serde_json::json!({ "savedHost": name, "sessions": sessions,
            "passwordsIncluded": false, "inputContentsIncluded": false,
            "physicalMouseAcceptanceClaimed": false }))
    }
}
