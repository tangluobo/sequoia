# Tomato Rust

This is a native Rust rewrite of the JavaFX project at `D:\Data\Git\tangluobo\tomata`.
Treat the Java checkout as a read-only reference. Work in this repository.

- Keep dependencies pointing inward: desktop/CLI → UI/services → core.
- Keep protocol code out of egui view functions. GUI and CLI share CommandHandler use cases.
- Use Repository for persistence, Adapter for protocols, Strategy for SQL dialects,
  Factory for sessions, and actor messages for background work. Add patterns only
  where they isolate real variation; prefer typed enums for a fixed set of states.
- Split views and controllers by feature. Avoid a single application/controller file
  accumulating every connection type and tool implementation.
- Match the original JavaFX interface, including geometry, assets, labels and interaction.
  Do not replace unfinished screens with a redesigned interface or claim visual parity
  before comparing the corresponding screen and states.
- Minimize runtime memory without changing appearance. Prefer event-driven repainting,
  visible-row rendering, bounded caches/history/results, and deterministic resource release.
  Measure release builds with the isolated memory probe and report working set separately
  from private commit. Never terminate user-owned running instances during probes.
- Preserve unknown legacy fields. The user explicitly requested sharing the original
  ~/.tomato/connections.json: load it directly and save in the Java-compatible encrypted
  format, keeping IDs, groups and credentials. Back up before the first change and
  reject saves when an external modification is detected. Never modify the Java checkout.
  Do not persist or log passwords in plaintext; exports remain redacted by default.
- Use RAII for sessions and server tasks. Keep potentially long blocking I/O out of UI frames.
- Update `docs/migration.md` accurately: registration, external delegation,
  partial implementation, and complete native implementation are different states.
- Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo test --workspace --locked`, and the CLI self-check for substantive changes.
- Network tests use isolated loopback fixtures, never saved user connections.

