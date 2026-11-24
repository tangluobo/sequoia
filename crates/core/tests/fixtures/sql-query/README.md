# Original named-query path oracle

`original.json` contains 23 path/name cases produced by `SqlQueryPathGolden.java`
against the unchanged `SqlEditorView.java` in the original Java checkout. The
oracle runs with a temporary home and does not connect to a database or read user
configuration. It covers Java ASCII `\s`, Unicode spaces, empty names, repeated
underscores and Java slash splitting. The Rust integration test compares the
paths independently of the platform separator.

Original source SHA-256:
`78B20C2A67DAFD0A4B7183F7F09C67D52E6C329C11543AF1D139A9F38E27A4BB`.
Fixture SHA-256:
`730B075C0B40FAF0F2C92A4EE0E7A199F8DB6F484A6B938815D041AC607B26D8`.

The repository adds bounded reads, version checks, atomic replacement and backup
protection. It rejects traversal, root-directory deletion and rename collisions;
these protections are not evidence of matching every original failure behavior.
