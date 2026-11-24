# egui-winit 0.36.2 local patch

Source: https://github.com/emilk/egui/tree/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui-winit

The normalized crates.io manifest, sources and README are retained in
`vendor/egui-winit`. Licensing is MIT OR Apache-2.0, with the same upstream texts
distributed as `eframe-MIT.txt` and `eframe-APACHE.txt`; both also reside in the
vendor directory.

In `src/lib.rs`, ordinary Ctrl+C and Ctrl+V also retain a Key event with the
event-time modifiers, before the existing Copy/Paste processing. This preserves
terminal controls when modifiers are released before the next GUI frame, and
retains Ctrl+V even without clipboard text. The SSH terminal sends the control
byte once and skips its accompanying clipboard event. It reserves Ctrl+Shift+V
for clipboard insertion, following the original Java TerminalView. Other widgets
continue receiving their normal Copy/Paste events; no empty Paste is synthesized.
