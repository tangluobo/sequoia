# SQLx MySQL/PostgreSQL 0.8.6 local patches

Both drivers retain the pinned upstream version and original MIT/Apache-2.0
licenses. No unsafe conversion, additional connection, or server write is added
by these patches.

MySQL columns retain the server's physical database, table and column names,
separately from display aliases. Shared `UStr`/`Arc` metadata avoids another copy
per row. PostgreSQL retains its existing relation OID and attribute number; the
application resolves those names after consuming the complete result stream.

Both completion objects expose shared rowset metadata. This preserves headings
and origins when a successful SELECT returns zero rows. PostgreSQL clears the
metadata at each statement completion so a following command cannot inherit
the previous SELECT's columns.

The isolated MySQL 8.4.11/PostgreSQL 17.11 probe
`target/database-fixtures/probes/wip-databases-metadata-33/validation.json`
passed eight native tests. Five metadata cases per driver compare aliases,
physical names, expressions, joins, empty results and JDBC type codes with the
unchanged Java `DatabaseService` and its original JDBC dependencies. This is
specific evidence, not a claim of complete SQLx driver or application parity.
