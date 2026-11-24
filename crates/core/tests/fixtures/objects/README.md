# Original object-browser formatting

`original-sizes.json` calls the original `DatabaseService.formatDataLength`
through `tests/java/DatabaseObjectsGolden.java`, with `Locale.US` and synthetic
numbers only. Its 18 cases include half-up rounding boundaries at KB/MB/GB/TB
and `Long.MAX_VALUE`. Zero is handled by the original caller as an empty string.

SHA-256: `F986509A7F66BDE133CFE0F0FBF55E6104EEFAF1F6C27B21ED47F29C402321F4`.

The helper neither reads saved configuration nor opens database connections.
