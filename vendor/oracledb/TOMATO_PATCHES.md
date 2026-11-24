# Tomato integration patches

Based on Oracle's `oracledb` 26.0.0-beta.4 crate (UPL-1.0 OR Apache-2.0).
Original copyright, LICENSE.txt and NOTICE.txt are retained.

- Apply the connect descriptor's transport timeout to direct TCP connects,
  redirected connects and initial read/write/TLS authentication operations.
- Parse TRANSPORT_CONNECT_TIMEOUT/TCP_CONNECT_TIMEOUT from full descriptors;
  default 10 seconds, bounded to 1–300 seconds.
- Reject packet lengths below their header or exceeding the negotiated buffer.
  Bound each accumulated protocol response to 32 MiB, including older servers
  which require additional packets while deserializing a response.
- Disable environment-triggered raw packet logging. Authentication packets
  must not be printed by the application.
- Expose a safe cloned-socket abort handle so dropping an actor interrupts a
  blocked driver read and releases the underlying session deterministically.
- Expose statement autocommit through the existing TTC execution option. The
  adapter disables it inside manual transactions and on ALTER SESSION.
- Owned loopback tests cover truncated/malformed packet headers, aborting a
  blocked read, and the 32 MiB response bound across appended packet batches.
- Bound OSON recursion (including cyclic offsets) to 64 levels and decoded
  node/string allocation to an 8 MiB budget before application conversion.
  Validate field IDs and checked byte-reader offsets instead of indexing blindly.
- Preserve the original error when timeout/reset recovery closes a connection.
  Implement the standard error source chain and retain typed driver errors in
  LOB I/O so the adapter can distinguish transport loss from SQL/data errors.
  Recovery policy remains in the application, which only replays typed reads.
- Read the LTXID and Application Continuity context using their double-length
  encoding, matching Oracle upstream commit
  `e258557c38f52467f3f59d727c22390ed80144ec` (`src/response/mod.rs`). Tests
  check that zero-containing contexts preserve the next message boundary.

The workspace excludes upstream examples/tests from default application builds.
Oracle adapters are still being integrated; vendoring is not proof of complete
application migration or successful real-database interoperability.

- Advance the packet-consumption cursor only after the complete announced packet is buffered. A timeout between header and body keeps the partial packet and its boundary intact. Owned TCP tests resume the same packet after a read timeout and verify the following packet.
