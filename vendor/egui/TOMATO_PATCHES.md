# Tomato viewport dispatch

`Context::show_viewport_app` registers a native deferred viewport without a
Send/Sync drawing closure. The matching vendored eframe integration dispatches
these passes to `App::logic` and `App::ui` on the GUI thread. This lets sessions
retain thread-affine native clipboard owners while repainting independently of
the parent window. Embedded backends return false so the caller can use the
existing immediate/embedded UI. Immediate registration explicitly restores the
Immediate class when a viewport changes registration strategy.

Logic-only passes push and restore the current viewport pair, just like UI
passes. Otherwise minimized child logic observes the root viewport and routes
session polling, repaint requests and window commands to the wrong window.

Native desktop input may query pointer_target_at for an individual raw press
position. It reuses egui's exact hit testing and layer interaction rules from the
last completed pass. This prevents floating-scrollbar input from reaching the
remote image and supports press/move/release sequences inside one frame, which
egui's frame drag snapshot deliberately does not classify as a drag. Ordinary
egui widget interaction and drawing are unchanged.

ViewportInfo optionally carries monitor_work_area_px in global physical pixels (unlike ordinary point-based rectangles). The winit adapter supplies the owning monitor work area on Windows; absent geometry preserves the existing fallback. take() retains this stable monitor metadata.
