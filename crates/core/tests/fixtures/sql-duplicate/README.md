# Original SQL duplication oracle

`original.json` contains 51 cases executed by the unchanged Java
`SqlEditorDuplicateSupport.duplicate`, through `tests/java/SqlDuplicateGolden.java`.
The fixture uses UTF-16 positions, which the Rust test translates at valid Unicode
boundaries. Cases cover empty text, line ends, LF/CRLF mixtures, Unicode and both
selection directions. Java files and the original application configuration were
not changed.

- Original source SHA-256: `2AD632F8F94F3B31C756B919EC147F2486E1F14C4FEF8030038DB31D0AFAC85E`
- Golden JSON SHA-256: `40ACD50B680AFED4073874D20A1CECEEB2936B21C212F26FAB582598D68C30D0`
- Oracle: GraalVM JDK 25, `--enable-preview --release 25`.

The generation arguments and logs are local artifacts under
`target/sql-duplicate-oracle`. The oracle does not connect databases or use the
system clipboard.
