# Access import/export adapter

`TomatoAccess.java` is a narrowly scoped adapter around Jackcess 4.0.8 (Apache
2.0), compiled ahead of time with GraalVM Native Image. The Rust application embeds
the host executable and extracts it into a private temporary directory **only**
while exporting `.accdb` or reading `.mdb`/`.accdb`. Users do not need Java, Access, Office, ODBC or ACE.
The process has a 128 MiB heap limit, receives a bounded typed disk stream, and is
killed and reaped when the Rust export control is cancelled. No credentials or SQL
are sent to the export path. Import database passwords travel only through stdin;
the source is opened read-only. Temporary data and executable are released after the job.

Build with `python scripts/build-access-export.py`, with GraalVM 25 `JAVA_HOME` and
Rust on PATH. CI builds one native helper per host before running Cargo. Maven
dependencies are pinned and checksum verified. The included Windows helper was
built with GraalVM 25.0.3; see `bin/*/build-info.json`. Source and third-party notices
are distributed with the application.

Access limits (255 columns, 64-character names, approximately 2 GiB database) are
enforced by Jackcess. Integers use exact NUMERIC(28,0), floating point uses DOUBLE,
booleans use nullable BYTE (0/1; Access YES/NO cannot preserve NULL), binary uses OLE, other values use Unicode LONG TEXT to avoid
silently losing date precision, decimal digits or long strings. NULL remains NULL.
The output is a new Access 2007 database; this adapter never edits an existing one.

Import uses Jackcess Encrypt 4.0.3 with Bouncy Castle 1.86 for supported Jet/Office
encryption. Plain/encrypted MDB and ACCDB are listed and streamed through a
bounded typed protocol. BigDecimal and integers retain their decimal text;
NULL, empty text and empty binary have separate tags. Attachments, multi-value
fields and version history use JSON values with binary payloads encoded in Base64.
The parent source worker's process job encloses the native helper so cancellation
also releases blocked/native decoding work. See `docs/data-import.md` and the
combined third-party notices for build, runtime and format boundaries.
