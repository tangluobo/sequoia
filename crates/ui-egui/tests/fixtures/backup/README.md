# Original BackupDialog selection behavior

`original-selection.json` comes from
`crates/core/tests/java/BackupSelectionGolden.java`, which invokes the original
`BackupDialog.buildObjectTree`, selection properties and selection extraction.
It records seven operation sequences and every intermediate result, including
partial selection, runtime-all selection and the source's retained custom-parent
flag. It creates no visible stage, starts no object loader and opens no database.

SHA-256: `7C6B7B638FE8A1E9D22B79D1F9C76C8DF22DD19264C6C037289BEC0742E9712F`.

`BackupDialogOracle.java` measures all four original tabs with five synthetic
objects after disabling the source loader. `RestoreDialogOracle.java` measures
the original restore window. `RestoreConfirmationOracle.java` invokes the actual
source confirmation, captures it and fires No before any database task is created.
Native viewport checks are in `scripts/qa/backup-window.ps1`; native controls are
also exercised against an owned loopback database by the optional UI harness in
`scripts/qa/isolated-databases.ps1`.
