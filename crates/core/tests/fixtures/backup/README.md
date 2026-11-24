# Original Java backup metadata and paths

`original-format.json` is emitted by `tests/java/BackupFormatGolden.java` using
the original Java `BackupService` private helpers and nb3 model classes.
It records 16 directory/filename cases and 17 SQL literal cases, plus the actual
PascalCase metadata and object definition. Windows failures for control or
trailing-space filenames are recorded as failures, rather than silently changing
the source behavior. All inputs are synthetic; no saved host is contacted.

SHA-256: `2E0AFC8D26716A10E0BEC82956A0B62E7596D43AF461AF8728536837230EF3D3`.

The matching Java TAR/GZIP container is in the services backup fixture directory.
Native scalar encoding intentionally fixes the source formatter's broken binary
and embedded-separator cases with valid SQL expressions. Successful source-file
and native-file restoration is verified separately against an owned MySQL server.
