# eD2k file-link downloads

The supplied `ed2k://|file|cn_windows_10_business_editions_version_1909_updated_jan_2020_x64_dvd_b3e1f3a6.iso|5311711232|3527D2A9845FF4105F485CC364655B66|/`
is a valid file link. Previously the UI and actor both called `url::Url::parse`
before dispatching every task to the HTTP adapter. Raw eD2k pipe-separated links
were rejected as `下载地址无效`; merely accepting the string would still leave no
download implementation.

`downloader/source.rs` now provides one typed parser for the dialog, public queue
validation, task creation and engine dispatch. It preserves the original URL,
decodes/sanitizes the file name, validates the 128-bit MD4 identifier and uses a
64-bit file length. eD2k file links of 1 byte through 256 GiB are accepted; server
links, malformed hashes, missing lengths and missing terminators are rejected
with protocol-specific errors. HTTP downloads retain their existing adapter.
BitTorrent is still unsupported and is not included in the input hint.

The embedded engine is adapted from MIT-licensed [Risuko eD2k](https://github.com/YueMiyuki/Risuko/tree/95875bfd7478fbb2beafa13e5d178454e361a029/src-tauri/risuko-ed2k),
pinned to `95875bfd7478fbb2beafa13e5d178454e361a029`. Source, license and adaptation
notes are retained in `vendor/risuko-ed2k`. It uses TCP server discovery, direct
file-link sources, client-only UDP Kad lookup, peer handshakes/capabilities,
upload-queue scheduling, 32/64-bit block requests, per-part MD4 verification,
corrupt-part rejection and verified-part resume. It does not launch Java,
an external downloader or a daemon. Protocol reference also includes the
read-only Java `DownloadProtocol`, `Ed2kDownloadEngine`, `GetFileSources`,
`PeerConnection` and the patched jed2k packet definitions.

Tomato integration patches include:

- Large-file capability advertisement and server source requests containing the
  zero marker plus the full 64-bit size. Peer block requests use the eMule I64
  opcodes when required; unsupported old peers are skipped.
- Direct TCP/UDP adapters, configured 1–32 outgoing peers per task, bounded
  channels, at most 300 pending source candidates, 4096 remembered addresses,
  256-entry Kad shortlist, bounded Kad queries/state and a bounded TCP frame
  decoder (including zlib-packed control frames).
- RAII server read/write task cancellation, explicit cancellation of peer/Kad
  work, an eight-client/5-second listener for server connectivity probes, and
  bounded shutdown waits off the UI thread.
- Stable native client identity and Kad state below the active profile's
  `rust-ed2k/`, allocated only when an eD2k task starts. Existing Java
  `server.met`/`server2.met` and version 0/1/2/3 `nodes.dat` are read without
  rewriting. The Java bootstrap URLs below `https://upd.emule-security.org/`
  refresh separate native server/node caches concurrently, bounded to 1 MiB each
  and six seconds, with cached/bundled fallback.
- Unique destination selection, per-task ownership sidecar and exclusive lease,
  verified-part rehash on resume, and completion that cannot replace a file
  created during the download. A corrupt or foreign nonempty partial is retained
  and reported, never silently deleted. Files grow as blocks arrive; adding a
  5.31 GB link does not preallocate or load a 5.31 GB file. Up to four blocking
  file workers use 512 KiB stacks and retire after two idle seconds; active task
  metadata is checkpointed every two seconds without periodic wakeups while idle.

The Java-compatible `downloads.json` container, `ED2K` protocol value, task fields,
unknown fields, first-mutation backup and external-modification guard remain in
use. These changes do not rewrite `connections.json` or saved credentials. Java's
in-progress binary resume format is not imported; its files are preserved and
the native engine uses its own `.part` / `.part.met` plus ownership marker.

Optional native networking overrides live in `rust-ed2k/network.json`:
`{"servers":["IPv4:port"],"kad":true,"loopbackOnly":false}`. With no overrides,
discovery uses the normal caches, server refresh, bundled servers and Kad.
Explicit server overrides replace the default server list. Isolated QA uses
only loopback servers with `kad:false, loopbackOnly:true`; this mode rejects all
non-loopback TCP destinations and never contacts the public bootstrap endpoint.

This is a download client, not a full eMule implementation: it does not provide
upload/seeding, encrypted-only peer transport, AICH repair, complete incoming
Low-ID callback transfers, Java partial-file import or BitTorrent. These remain
separate migration gaps. Availability and speed of a public ISO depend on online
sources and their upload queues; accepting a hash link does not establish that
those sources exist.

Validation evidence is recorded below after final build and native acceptance.
Protocol tests never contact saved user connections or public ISO sources.

Development acceptance (2026-10-24): final formatting, strict all-target workspace
Clippy, all-target build, CLI self-check and read-only legacy configuration check
passed. The workspace run passed 728 tests with 82 environment-dependent tests
ignored, across 54 summaries (`target/downloader-ed2k-checks-1563.json`). The
configuration check read the current 60 connections and four groups, without
connecting to saved hosts.

Independent TCP fixtures verified server source lookup, actual 9,990,145-byte
transfer, MD4 verification, cancellation, actor close/reopen, verified-part resume,
exact output bytes, old history/unknown-field preservation and socket release.
The supplied 5,311,711,232-byte link passes the common parser; a real TCP fixture
checks its 64-bit source and peer request layouts without preallocation. The
complete public ISO was not downloaded. The embedded protocol suite also covers
Kad bootstrap/lookup, queue timing, corrupt parts, wide offsets and state bounds.

Six development native windows passed with normal exits in
`target/window-probes/downloader-ed2k-dev-1565/validation.json`. The actual Add
dialog transferred the RFC 1320 `abc` file through eD2k discovery and a peer, with
MD4 `A448017AAF21D8525FC10AE87AA6729D`, exact output SHA-256 and zero retained
sockets. The protocol column is explicitly checked as `ED2K`, after framebuffer
inspection found an older hardcoded HTTP label. All network fixtures are local;
these are application-owned egui inputs and GPU framebuffers, not OS input.

An initial full run under concurrent release compilation exposed cancellation
and timeout becoming ready together. Kad requests now give cancellation priority;
the fixture uses receipt of the real UDP request as its cancellation barrier.
The failing initial log is retained alongside the passing final full run.

The shared connections file changed at 2026-10-24 18:33:08 local time while this
task was working. Its newest bytes were preserved; this task did not write that
file. The before/after metadata and unidentified writer are recorded in
`target/downloader-ed2k-config-concurrency-1564.json`. Other protected inputs retain
their original hashes; no rollback of the concurrent configuration was made.

Release acceptance (2026-10-24): `target/qa-candidate-downloader-ed2k-1564/tomato.exe`,
default optimized release with opt-level 3, thin LTO and 16 codegen units,
143,468,032 bytes, SHA-256
`66976BF4E700EFA1DD71211E0DC6D716C221875B8E5A47953593D53CF2506E75`. The compiler invocation and exact Cargo artifact are
recorded in `release-profile.json`; no debug executable was substituted. All
2313 production fingerprints and all additional Rust test fingerprints match
the final freeze. The same immutable EXE passed all six actual GPU window cases,
HTTP/eD2k output hashes, peer/server socket release, normal exits, original icon
frames and embedded Access/ODBC worker hashes. Four main window frames are pixel
identical to development; child dialogs match outside owned temporary directory
and caret fields. An initial comparison mask ended before the settings path
field's right edge; its 77 differing pixels were solely the fixture GUID text,
and the corrected exact field mask passes. No application style change was made
to accommodate that comparison.

Same-batch idle memory uses independent empty profiles, a 15-second warmup and
ten one-second samples per build/backend. Other user-owned instances were kept.

| Backend | Working set MiB (1542 → 1564) | Private commit MiB (1542 → 1564) |
| --- | --- | --- |
| auto | 93.55 → 93.30 | 128.54 → 128.29 |
| vulkan | 92.61 → 94.82 | 128.35 → 130.52 |

These are idle native-window measurements, not an active-transfer peak or a
claim of lower memory in every workload. eD2k/Kad initializes only for eD2k tasks;
no full ISO is held in memory. Evidence:
`target/window-probes/downloader-ed2k-memory-1564/validation.json`.
