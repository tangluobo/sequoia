# eframe 0.36.2 local patch

2026-10-23 startup: `NativeOptions::wgpu_preferred_backend` is opt-in and defaults
to `None`. Native WGPU initialization can try one API before creating the
original full configuration. Adapter/device/surface errors and software-only
preferred adapters fall back after releasing the first painter. Existing
setups and custom selectors keep upstream behavior. The initial root remains
hidden until painted; diagnostic milestones do not enable idle polling.
See `src/native/wgpu_initialization.rs` and the native startup regression in
`apps/tomato/tests/startup.rs`. No shader or surface presentation parameters change.

Source: https://github.com/emilk/egui/tree/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/eframe

The normalized crates.io source, `src`, `data`, manifest and README are retained in
`vendor/eframe`. Upstream licensing is MIT OR Apache-2.0; both texts are included.

Modified `src/native/wgpu_integration.rs`: the immediate native-viewport renderer
previously passed an empty capture-command list to wgpu and left clipboard actions
unprocessed. It now services screenshot commands and collects completed captures,
and queues Cut/Copy/Paste events with the same CRLF normalization as the root
renderer. Queued clipboard input requests a repaint. This enables the native
Markdown find window's event path and its opt-in framebuffer QA; normal idle
windows add no periodic polling.

Immediate viewport visibility also keeps root UI execution available while a
descendant native window remains visible. The root minimized-window sleep is
skipped in that case, so an independent RDP window does not inherit a minimized
main window's artificial frame delay. Fully invisible trees retain the throttle.

2026-10-22: The WGPU and Glow root paths also execute one non-painted UI pass
when native viewport information contains a close request, including an entirely
minimized tree. Application-owned child logic can dispose its session without
painting; the parent's UI pass then removes its registration so native windows,
input state and GPU surfaces are collected. Ordinary hidden ticks still use
logic-only updates and retain the existing throttle. This does not alter root
close confirmation or force an unminimize/focus action.

An accepted child Close explicitly schedules its parent's native update rather
than relying solely on egui's coalesced repaint callback. The parent can then
retire the child even when its earlier repaint was consumed or both windows are
minimized. CancelClose keeps the child. WGPU also delivers commands for surviving
viewports if the currently rendered child was already pruned, so parent Close
commands are not discarded by the early return. Debug diagnostics identify native
viewport retirement without including connection credentials.
