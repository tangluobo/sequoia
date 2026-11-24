# egui 0.36.2 local patch

Source: https://github.com/emilk/egui/tree/49682f8baa058bf49e011035cfbd6e825f88a5ef/crates/egui

The normalized crate source is retained in vendor/egui, under MIT OR Apache-2.0.
License texts are included there and distributed as eframe-MIT.txt and
eframe-APACHE.txt.

Undoer exposes a retained-state memory estimate and trimming operation. Trimming
discards oldest undo states, then farthest redo states, without cloning snapshots.
TextEditState exposes this through its existing shared undo mutex. Document
editing commands take and return the existing undo history instead of deep-cloning
all retained document snapshots for each undo, redo or replacement operation.
Document editors enforce a 32 MiB retained-snapshot budget. This includes the latest edit
flux, but excludes container metadata, the live text buffer, layout and renderer.
Recent undo/redo remain usable; exceptionally large individual snapshots can be
discarded. Upstream default behavior is unchanged unless the caller opts in.

TextEdit additionally exposes an optional event editor, called in the widget's
ordered event loop after each earlier text/IME edit has updated the buffer. An
application can return a replacement and scalar cursor range; these edits use
the existing undo history and re-layout path. This is used by Markdown's source
compatible four-space Tab, list continuation and rectangular replacement rules.
The opt-in pointer_selection switch suppresses the competing linear mouse
selection while the application owns an Alt-drag rectangle. Neither extension
changes widgets that do not opt in.

Opt-in document editors also compare undo content independently of caret movement.
Undo/redo skips caret-only checkpoints while restoring the saved selection of the
target edit. Automatic history feeds construct an owned snapshot lazily, avoiding
two complete document copies per idle frame. The generic Undoer defaults and text
widgets without an event editor retain their original comparison behavior. SQL
toolbar formatting followed by mouse navigation, undo and redo has an actual
widget regression; the lazy feed additionally asserts that unchanged content
never requests another owned snapshot.

Read-only TextBuffers now skip the two automatic undo snapshot feeds entirely.
They still use the original selection/copy event handling. An actual widget
regression selects and copies a multi-line SQL information document and asserts
that its retained undo memory remains zero across frames. Editable widgets keep
their existing history behavior.


2026-10-18: the application event hook now returns `EventEdit::Selection` or `EventEdit::Replace`. A consumed navigation/command event can update the selection without allocating a copy of the document. Replacement still runs through the same ordered input stream, records content undo and enforces the caller's undo-memory budget. SQL navigation/save/AI/run and rectangular Markdown IME/error handling use the selection variant; default typing and ordinary IME handling are unchanged. All three application hook call sites were updated together. Full workspace verification is recorded in migration.md.
