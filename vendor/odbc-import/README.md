# Windows ODBC import adapter

`odbc.c` is Tomato's small stdin/stdout protocol adapter for installed Windows x86
ODBC drivers. It is released under the repository's MIT license. It statically
links the CRT and dynamically uses the Windows ODBC driver manager (`odbc32.dll`).
It does not redistribute Microsoft Access/Jet/ACE or any third-party ODBC driver.

Build with `python scripts/build-odbc32.py` on Windows with Visual C++ x86 tools.
The GUI embeds the resulting executable, extracts it only during an ODBC import,
and encloses it in the source worker's kill-on-close job. Passwords and connection
strings use stdin; profiles and diagnostics do not contain them. Output is bounded
to 16 MiB per row. SQL identifiers are driver-quoted and queries use a 30-second
driver timeout. A blocked driver can be stopped by cancelling the parent worker.

The x64 path uses the Rust `odbc-api` crate directly. Users choose the driver
architecture in the wizard; x86 and x64 DSNs have distinct lists.
