# Original TableObjectsView interaction evidence

`original-selection.json` comes from
`crates/core/tests/java/DatabaseObjectsWindowOracle.java` in `SELECTION` mode.
It uses the unchanged original Java view, its loader and selection/filter
methods. A JDBC proxy is injected into this probe's private in-process cache;
all table, view, comment and foreign-key values are synthetic. It opens no
sockets, reads no saved configuration and never writes the system clipboard.

Eight action sequences record every intermediate visible/selected list,
insertion order, primary object, anchor and status text. They include Ctrl+Shift,
selection removal, select-all, a temporarily filtered-out anchor, Unicode
whitespace and comment search.

SHA-256: `BA6CAF6D84EFBD4D27A6721A0FD5266D852707E6967C23344E11B0FE10414B21`.

The same oracle captured the actual original 896×650 ICON/DETAIL/ER scenes under
`target/ui-objects-original-108-*.png`. These are original-view layout evidence,
not real database adapter tests or proof of native UI equivalence. Actual native
MySQL/PostgreSQL catalog checks run separately in `database_servers/objects.rs`.
