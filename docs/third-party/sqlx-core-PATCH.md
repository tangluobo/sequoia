# SQLx core 0.8.6 local patch

The original MIT/Apache-2.0 source is copied unchanged from the already pinned
Cargo package, except for `AnyConnection::backend_mut`. Its existing backend
trait already extends `std::any::Any`; the accessor safely borrows the concrete
connection using stable trait upcasting, without unsafe casts or another socket.

This lets the application retain one transaction/session per connection and
database while reading native MySQL/PostgreSQL result values. SQLx Any's limited
type conversion can otherwise reject dates, exact decimals, JSON and other types
before the application's row converter can inspect them.

The accessor requires Rust 1.86 or newer; the workspace already requires 1.95.
Trait upcasting stabilization is documented in the
[Rust 1.86 release](https://blog.rust-lang.org/2025/04/03/Rust-1.86.0/).

The patch and native decoder are registered. Owned loopback MySQL 8.4.11 and
PostgreSQL 17.11 fixtures cover workbench changes, independent database scopes,
transaction rollback/disconnect, and result strings compared with the original
JDBC dependencies (`target/database-fixtures/probes/wip-databases-17`). MySQL BIT,
decimal and floating display also use Java-generated golden cases. These checks
do not establish every native type, connection option, or full workbench parity.
