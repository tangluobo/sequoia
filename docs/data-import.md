# Data import wizard

The table toolbar and database objects import action open a native 680 × 480
wizard. Database backup restoration remains a separate action. The source-format
page follows the supplied Navicat screenshot: DBF, Paradox, text, CSV, Excel,
XML, JSON, Access and ODBC, with text selected by default.

The subsequent pages choose files or an ODBC source, configure decoding and record
ranges, preview source rows, select new/existing destination tables, map fields,
choose the write mode, review options, and show execution progress. Multiple
files, Excel sheets and Access tables can be selected in one import. Field
mapping supports name/order matching, independent inclusion and key checkboxes,
new-field SQL types and constant defaults. An unmapped field without a constant
uses the database default. Empty strings and NULL are distinct.

Six write modes are provided: append, update matches, append/update, skip matching
keys, delete matches, and transactional copy. Updates and deletes require keys
that uniquely locate a target record. Copy confirms target names and deletes
within the same table transaction as insertion. Identifiers are dialect-quoted;
values use bound parameters. Existing views and generated columns cannot be
written. Target structure is checked again before execution.

By default each table is a transaction. The optional batch size commits groups
of records instead. Continuing after errors uses per-row savepoints on
transactional tables. Cancellation rolls back the current uncommitted transaction;
previously committed batches/tables remain. Commit/rollback timeouts report an
unconfirmed result and never automatically retry. New-table DDL can leave an
empty table after a failed import. Nontransactional engines cannot provide rollback
or copy semantics; use batch mode and review partially written data on failure.
Related open table/object/tree views are refreshed after confirmed or uncertain
changes, respecting existing dirty-editor guards.

## Source adapters and boundaries

| Source | Reading and configuration |
| --- | --- |
| Text/CSV | Streaming quoted/multiline records, separators, text qualifier, header row, start/end records, NULL marker, trimming, BOM and selectable character encoding. A quoted NULL marker remains literal text; quoted empty text is retained. Only quote flags are kept alongside the CSV decoder, without duplicating row payloads. |
| DBF | dBASE/FoxPro reader with header language-driver decoding or selected encoding, deleted-row handling, memo companions, and raw numeric/currency decoding to avoid double rounding. |
| Paradox | Independent documented table/block and companion MB reader for unencrypted Paradox 3–7 tables. Alpha, numeric, date/time, logical, bytes, BCD and memo/blob types. Encrypted tables require an installed suitable ODBC driver. Invalid records fail explicitly. |
| Excel | XLS/XLSX sheets, cached formula values, dates, blank cells and record ranges. Select sheets by name or leave blank for all. XLSX sheets are requested individually; the XLS reader can cache the decoded workbook inside the source worker. Limits are 64 MiB compressed file, 256 MiB expanded XLSX, 20,000 ZIP entries and 4 million cells per sheet. Large workbooks should be split or converted to streaming CSV. |
| XML | Auto row/record/direct-child nodes or an explicit node path; attributes, CDATA, escaped text, nested content, NULL and hex/Base64 binary. DTDs are rejected. |
| JSON | Root objects/arrays, selected dot or JSON-pointer paths, JSONL/NDJSON, union of fields, exact numeric strings and nested JSON values. |
| Access | Read-only MDB/ACCDB with native Jackcess and Jackcess Encrypt; optional database password. No Java/Office/ACE installation at runtime. Jet and supported Office encrypted files use stdin credentials. Complex values use JSON, attachments retain Base64 data. |
| ODBC | Installed x64 drivers through `odbc-api`; Windows also embeds a small x86 bridge for 32-bit Jet/ACE and other installed drivers. Enumerates architecture-specific DSNs/drivers/tables, supports credentials or a connection string, selected tables or a single SELECT query. Drivers themselves are not bundled. |

Source parsing runs in an owned subprocess before loading any saved connections.
Cancelling terminates its process tree, including native Access or x86 ODBC helpers.
Parsed rows are streamed to private temporary JSONL snapshots and removed on
completion/close. A maximum of 100 files/tables, 16,384 fields, 16 MiB per row and
100 preview rows per source table is enforced (the row limit applies to its encoded cache representation); aggregate previews are capped at
8 MiB. CSV/JSON/XML decoding budgets reject oversized records during parsing.
Temporary source data is not a saved connection configuration; keep the operating
system temporary directory on a disk appropriate for the data being imported.

Saved `.timport` profiles include mappings and options but remove Access/ODBC
passwords, ODBC connection strings and stale target definitions. Loading a profile
re-reads the source and destination and resolves saved fields by name. Import
logs omit source field values and driver error details that can contain secrets.
The legacy encrypted `connections.json` format and its unknown fields are unchanged.

Profiles identify each source table by its origin and name, so a reordered
workbook/file list cannot silently swap saved destinations. Missing source tables
are deselected for review. Source fields are also resolved by name. The mapping
grid keeps a fixed 26-point row height while selectors are opened.

Windows worker polling deliberately reads only the root process handle. The
owned job is terminated/reaped once, preserving the completion notification for
descendant cleanup. This avoids an upstream `process-wrap` standard-wrapper
race in which `try_wait` drains `ACTIVE_PROCESS_ZERO` before a later job wait.
Native Access/ODBC helper directories live inside the parent's owned source
cache, so cancelling the worker also removes partial helper outputs. The
`import_source_fixture` example repeats the real process protocol or cancels
within a native helper workflow and checks that the owned cache is empty.

## Build and validation

Build the pinned native Access adapter with `python scripts/build-access-export.py`
(GraalVM 25), and on Windows run `python scripts/build-odbc32.py` (Visual C++ x86).
Then use the normal Cargo workspace checks. CI and release workflows build the
adapters before Cargo embeds them. Jackcess Encrypt/BC notices are included in
`docs/third-party/access-export-NOTICES.txt`; the ODBC bridge source is MIT.

`scripts/qa/import-adapters.py` checks native Access values/encryption and installed
Windows text ODBC against local owned files. `scripts/qa/data-import.ps1` drives
the real wizard's widget events and captures its native framebuffer; use
`-ScreensOnly` for every wizard page. The ordinary tests exercise mapping and
six actual SQLite write modes, constraints, rollback, defaults, exact decimals,
encoding, source ranges, oversized records and credential redaction. An explicit
ignored test can read the independent public Paradox reference fixtures via
`TOMATO_TEST_PARADOX_DIRECTORY`. These fixtures never contact saved hosts.

Release `import-1485` passed 571 default workspace tests (82 environment-dependent
tests remain ignored), strict Clippy, formatting and the CLI self-check. The exact
release EXE passed all seven native wizard pages and ten end-to-end file cases
(XLS and XLSX separately), importing 23,050 rows with independent Python SQLite
readback of every row. MySQL 8.4.11 and PostgreSQL 17.11 each passed all six write
modes in owned loopback instances. The release also passed 24 native export cases
and six native workflows against the unchanged Go Cloud server.

Five public encrypted MDB/ACCDB samples passed the unchanged release worker in
isolation. An earlier concurrent direct-worker probe observed a transient Windows
sharing violation when immediately deleting its extracted helper EXE; its failed
diagnostic is retained. Normal GUI source caches were removed on exit in all ten
release import cases. The separate lifecycle fixture passed twenty reads each of
CSV, Access and ODBC, plus cancellation inside both native helper workflows.
These checks establish the tested workloads rather than a universal file-size,
driver or destination-server guarantee. Per-release evidence is bundled under
`validation/import/` with hashes and limitations in `build-info.json`.

Reference behavior: [Navicat import/export](https://www.navicat.com/en/products/import-export?product=navicat-for-mysql),
[key requirements for update/delete modes](https://help.navicat.com/hc/en-us/articles/218301277-How-do-I-enable-all-the-import-modes-available-in-the-Import-Wizard).
Paradox format references: Randy Beck's [format description](https://www.randybeck.com/paradoxformat.shtml)
and Kevin Mitchell's PARADOX4 format notes (1996). The implementation does not link
the separately inspected LGPL Java reference driver.
