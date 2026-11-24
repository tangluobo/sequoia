# Original SQL formatting results

Twenty cases invoke the private `formatSql` method of the unchanged original
`SqlEditorView.java` using `SqlFormatGolden.java`. Constructor-free allocation
avoids GUI/database initialization and the oracle runs with an isolated home.
It does not read saved connections or call an external service.

The source method performs ASCII whitespace normalization and case-sensitive
keyword replacements, including inside quoted SQL. The Rust migration preserves
those results rather than substituting another formatter's behavior.

Source SHA-256:
`78B20C2A67DAFD0A4B7183F7F09C67D52E6C329C11543AF1D139A9F38E27A4BB`.
Fixture SHA-256:
`ED9CA3390010CD7D5D3EB226E07B0EFEEB7E17BB7E38E970063A9550034735FE`.
