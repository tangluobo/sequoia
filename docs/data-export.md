# Data export wizard

The table toolbar and both table/result context menus now open a native scope
confirmation: **全部记录 / 当前 N 条记录 / 取消**. The five-step native export wizard
then selects format, file, fields, format options, and reports progress/result.
The format order, default TXT selection, compact buttons, blue heading, dimensions
and row positions follow the user's Navicat screenshots. This is a data exporter;
the object browser's export command no longer opens the database backup screen.
Object-browser exports require selecting one table or view and export its records.

Current records use the visible page/result projection, column visibility/order,
NULL bitmap, pending edits and pending row deletions. Opening the wizard commits
the active cell editor into the existing draft; it does not write changes to the
database. The current snapshot is moved into the worker, not cloned again at start.
All table records use a new session with the original database, schema, table and
server sort; they are independent of the 1,000-row table page cap. Current-page
search/column filters are temporary local projections and are applied to **current**
exports. All exports read saved database records. A changed table column layout
after opening the wizard is detected instead of silently applying old field indices.

Query exports retain the result's execution connection and original statement,
including when the editor or connection selector has subsequently changed. The
all-record path re-reads a single result query on a dedicated read-only session.
The exporter does not rerun a multi-statement script or a modifying statement.
The current-results path remains available when a result cannot be reread.

| Format | File implementation | Relevant boundaries |
| --- | --- | --- |
| DBF | Native Rust dBASE III, GBK language driver | 128 fields, 10-byte field aliases, 254-byte character fields; a mapping editor provides short aliases. Values remain character data, binary is hex, NULL uses the configured marker. Unsupported text/lengths fail without truncation. |
| TXT | Native Rust UTF-8 delimited text | Selectable separator/BOM/header/NULL marker; quoted strings distinguish NULL from empty strings and literal markers. |
| CSV | Native Rust RFC-style quoting, UTF-8 | Default comma separator; quotes, embedded CR/LF and Chinese preserved. |
| HTML | Native Rust UTF-8 HTML table | Escaped values; explicit `data-null` metadata; no active formulas/scripts. |
| XLS | Native Rust BIFF8 + OLE CFB v3 | Disk-backed SST/worksheet streams; 65,536 rows including header, 256 columns, 32,767 UTF-16 code units per cell. Large integers stay exact text. |
| XLSX | Native Rust streaming OOXML ZIP | Inline strings avoid a shared-string heap; 1,048,576 rows including header, 16,384 columns, Excel cell-size limit. CR and literal OOXML escapes are encoded explicitly. |
| SQL | Existing dialect strategy, one INSERT at a time | Quotes identifiers and values, preserves SQL NULL and binary literals; source dialect retained. |
| XML | Native Rust XML 1.0 | Fixed `record/field` structure supports arbitrary SQL column names; NULL and hex binary are explicit. Invalid XML characters cause an error. |
| JSON | Native Rust streaming object array | Explicit NULL, exact integers, escaped strings, hex binary; duplicate field names are rejected rather than overwritten. |
| ACCDB | Embedded native Jackcess adapter | Real Access 2007 file, no installed Java/Office/ODBC/ACE. Jackcess encodes the file in an isolated on-demand process; this format is external library delegation, not a pure Rust Access writer. |

Excel values beginning with `=` remain literal strings. Unsupported format limits
produce a visible error and retain an existing destination. File extensions must
match the chosen format. The wizard is not a full clone of Navicat's saved-task,
template, database creation, or schema-conversion features.

## Resource and cancellation behavior

Database rows are streamed into a private disk spool. A 64 KiB buffered writer and
one parsed row are retained; no complete-table `Vec` or workbook shared-string map
is created. A row's serialized representation is limited to 32 MiB. Output is built
in a temporary sibling file, synced, and published only after success and a final
destination revision check. Cancellation/errors remove the spool and partial output.
An existing destination modified by another program during export is preserved.

Access extraction and execution happen only when exporting Access; its native
worker uses a 128 MiB heap limit and is killed/reaped on cancellation. The worker
receives only typed export data, never connection credentials or SQL. Normal app
shutdown cancels the export and awaits cleanup concurrently with other resources.

## Validation

- Service tests generate and reopen all ten file types. Calamine independently
  reads XLS/XLSX, dbase reads DBF, SQL is restored into a fresh SQLite database,
  XML is parsed and JSON is decoded. Access is reopened with Jackcess.
- Fixtures cover Chinese, quoted CR/LF text, NULL/empty/literal NULL, binary and
  empty binary, maximum signed 64-bit integers, formula-like text, long BIFF8
  Unicode continuation records, >1,000-row all/query exports, changed destinations
  and cancellation during spooling. Existing outputs survive failure/cancellation.
- Native viewport bridge tests exercise scope buttons, all ten radio choices,
  back/next/cancel, validation, actual background export, and label alignment.
- `scripts/qa/data-export.ps1 -AllFormats` captures real native child viewports
  and clicks through scope/format/file/field/options/start using only app-owned
  egui events. It uses an isolated file database (5,003 rows; current page 1,000),
  never saved hosts or OS input, and requires normal process exit and cleanup.

Release acceptance, screenshots, exact check counts, binary hash and paired idle
memory measurements are recorded in the candidate's build information and
`target` QA proofs. No legacy configuration schema or font settings are changed.


### Final local acceptance (2026-10-24)

Release 1370 (`9126A50E16EEA1077F26D5A48399A84D222B66D82057037AB8864929BDB0A133`)
passed 546 workspace tests, strict all-target Clippy, formatting and CLI self-check.
All 24 native cases completed normally; all 20 output files were independently
read with 1,000 current / 5,003 all records. Three static native frames are pixel
identical to the reviewed development captures. Access edge cases additionally
round-tripped exact signed 64-bit extremes, Chinese/emoji/CRLF, NULL/empty/literal
markers and binary/empty binary through a normal-JVM reader after native writing.
A separate run with JAVA_HOME/GRAALVM_HOME removed and only System32 on PATH still
created a real Access file. Legacy profile files, icons and previous EXEs remain
unchanged; Cargo.lock intentionally adds export file-format dependencies.

The supplemental shutdown check retains one existing disconnected-desktop DX12
fullscreen timing limitation shared by the previous release; see performance.md.

## Export field list correction (1619, 2026-10-25)

Field checkboxes now share a left edge, with 22-point rows independent of global
spacing. The bounded list fills the area above the footer, scrolls vertically and
horizontally, keeps normal-format field names on one line, and displays the selected/total
field count. Solid scrollbars remain visible whenever scrolling is needed. DBF source
name cells have a fixed width with ellipsis and a complete-name tooltip, so long
names cannot displace the alias column. Select all/unselect all affect the complete schema, including rows
outside the viewport. DBF field aliases remain editable beside the source name.
Only visible rows create widgets; the original export selection pipeline and
connection file format are unchanged. Layout reference: [Navicat official export
example](https://www.navicat.com/en/company/aboutus/blog/2027-exporting-mysql-tables-to-csv).

Validation: 771 workspace tests passed, zero failed, 84 conditional tests ignored;
format, strict all-target Clippy, CLI self-check and read-only configuration check
passed. Real widget input scrolls to field 256, toggles its export selection, and
selects/unselects all offscreen fields. Five final EXE native child viewports cover
4/256 fields, top/bottom scrolling and DBF aliases, with normal exit. Native
fixtures use isolated SQLite and in-process egui input, never OS input or saved hosts.

| Backend | Working set MiB (1616 → 1619) | Private commit MiB (1616 → 1619) |
| --- | --- | --- |
| auto | 88.19 → 87.87 | 119.46 → 118.95 |
| vulkan | 87.27 → 87.42 | 119.09 → 119.36 |

Paired idle windows, 15-second warmup and 10-second sampling. Working set and private
commit are separate; these numbers do not measure active transfer or export peaks.
Freeze `export-fields-1619-final`; EXE SHA-256 `C964C80D84BDEA9F0C917FDB77BDE4D8DF94D846F62FA86F36CB65A2C3FEB34A`.
