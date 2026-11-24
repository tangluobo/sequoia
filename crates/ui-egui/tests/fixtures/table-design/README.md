# Original table designer actions

`original-actions.json` is produced by `crates/core/tests/java/TableDesignStateOracle.java` using the unchanged Java `TableStructureView`. The oracle invokes actions on its own JavaFX controls, using a synthetic JDBC connection and in-memory settings. It does not connect to saved hosts, write configuration, use OS input, or write the system clipboard.

Each intermediate snapshot includes all 15 source field values, selected cells, focus, dirty state and footer text. The native state model is currently registered for verification only; the complete production view/controller remains unfinished. Clipboard cases inject only the view's private cached payload before invoking the original paste action.

The fixture currently contains 7 sequences / 42 intermediate states. `copyJson` is a pure Gson projection of the source view's actual selected rows and column titles; it does not call the OS-writing copy handler. SHA-256: `6E160CB8C30AD9F34BE8E52C40CF97A2E1D29EC21729D047E6F1A8CF3BA6DDEA`. The native decoder limits payloads to 8 MiB and drafts to 4,096 fields; nonstandard flag strings and numeric/boolean scalar spelling are preserved. Normal rows store only a compact flag mask; extra strings are allocated only for nonstandard flags.
