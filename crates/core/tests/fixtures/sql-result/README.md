# Original result widget cases

`original-sort.json` executes the original JavaFX result TableView sorting and
selection model. Its source helper is `tests/java/SqlResultSortGolden.java`.
Clearing the sort order retains the last item order; selection tracks identity.

`original-scrollbars.json` records the actual styled original result widget's
13-pixel bars, 10-pixel increment and 420-pixel page in a 700×450 owned scene.
The helper is `tests/java/SqlResultWindowOracle.java`. The original stylesheet
is inherited from SqlEditorView; the earlier unstyled Modena capture is superseded.

`original-edit-plan.json` invokes the unchanged original QueryResultEditPlan and
ConnectType compiled directly from the read-only Java checkout. It contains 100
cases for physical origins, expressions, aliases, primary keys, original-row
matching and Java Unicode case comparison. The helper is
`tests/java/QueryResultEditGolden.java`; fixture SHA-256:
`2A466AEB7A28384EF8FE64A9FFE21727B73F1CB64BB4EC167E085A213A687BF1`.

These oracles establish the tested model/widget behavior. They do not establish
complete SQL toolbar, query editing, context-menu or workbench parity.

`original-transfer.json` calls the unchanged original TableCellContextMenuUtils
methods returning JSON, camel-case JSON, comment/value text and TSV (including
column reordering and hidden columns). It does not invoke the OS clipboard.
Fixture SHA-256: `134D6AE034D63486CBC35558D66AA9139B08D01E4301C38E5E7CE1EAD7D43697`;
original helper SHA-256:
`1ABDE5E94CEF540EFB24240026F8EC517F00A536DC4B9BB4600331FEABF44D8C`.
The Rust renderer has separate input tests; the helper-only SQL formatting test
is not evidence that the original disabled Update menu item should be enabled.

`original-filters.json` invokes the unchanged Java comparison and condition parser
for 1936 pairs × 6 operators, including NULL, case-sensitive literal containment,
BigDecimal scales/exponents, and BMP decimal digits. Fixture SHA-256:
`1E3089BCF78EDAF4F8919FA9E72BDC153D14CE49C5CB5FC752CD2C9C1E9E7763`.
The helper is `tests/java/SqlResultFilterGolden.java`. This proves comparison
semantics; it does not by itself prove the interactive filter menu or dialog.

`original-view.json` records combined sort/filter/selection/deletion and physical
column reordering on the actual original result TableView. Its helper is
`tests/java/SqlResultViewGolden.java`; it uses neither saved hosts nor the OS
clipboard. In particular, clearing all filters restores the saved source order
and clears sort order; deleting a filtered row removes the physical source row.

Combined-view fixture SHA-256 (21 transitions, including tied sort values and
all data columns hidden):
`9373F88AE11C20438D21593F907FF163425A2C50171EA0FEE68FDD09E3059C59`.
Changing comparators on a filtered list preserves the previous relative order of
tied rows. The golden is a model/control oracle, not a complete workbench proof.

`original-temporal.json` invokes the original QueryResultEditableTableCell's
private datetime parser, numeric initializer and formatter: 30 datetime strings,
19 numeric strings, and all five temporal JDBC codes. Fixture SHA-256:
`64BA997F071A6DD7EC78A95404707D3A5695F230BB502432EC933113333A2BFD`.
`tests/java/SqlTemporalOracle.java` also opens the original calendar/time popup
in an owned scene, with no database, clipboard or desktop input access.
`tests/java/SqlResultDialogOracle.java` measures the original 352×154 custom
filter dialog. Native Rust popup snapshots use an isolated SQLite actor and
explicitly injected editable cell metadata; they do not prove a popup-to-server
write roundtrip. Real MySQL/PostgreSQL writeback is validated separately.
