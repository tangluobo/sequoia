# Tomato rendering patch

Based on crates.io ironrdp-client 0.1.0 (upstream identity in .cargo_vcs_info.json), MIT OR Apache-2.0.

Retain protocol and Windows clipboard ownership. Coalesce GraphicsUpdate rectangles for 16 ms, publish only the dirty region, and cooperatively yield after a 4 ms decode slice. Preserve the upstream full-image event for API compatibility. This avoids converting and copying an entire desktop for every scanline PDU.

Direct TCP transports also enable TCP_NODELAY immediately after connecting, as the
gateway transport already does. Small mouse/keyboard messages should not wait on
Nagle/delayed ACK interactions. Cursor shape transport remains upstream; Tomato
disables server-image software cursor compositing and paints the latest shape
locally. Loopback measurements do not claim real-host latency or frame rates.

Linux broker redirection (2026-10-22): recognize bounded enhanced-security Server Redirection packets before generic Share Control decoding; reconnect with the server-issued session and routing data, and perform native RDSTLS v1 where requested. Preserve encrypted passwords byte-for-byte, bound hops to six, and retain the original broker configuration lazily for later size reconnects. Redirection state is transient and never written to saved connections. See docs/rdp-linux-redirection.md and the private wire-vector tests.
