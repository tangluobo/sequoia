# Tomato compatibility patch

Upstream: `ironrdp-connector` 0.10.0, IronRDP commit
`11a0810cfbbabd8b8023875a05e3041216d4b01b`,
`crates/ironrdp-connector`. The original manifest and both upstream licenses are retained.

Changes:

- `sspi` 0.21 → 0.23 and `picky` rc.25 → rc.26, matching SSPI's exact dependency.
- Propagate the now-fallible `TsRequest::buffer_len()` result before allocating the output buffer.
- Remove a stale `single_use_lifetimes` lint expectation that is unfulfilled with Rust 1.99.

This allows current russh 0.64.1 and its fixed allocation handling to coexist with
IronRDP. Cryptographic algorithms are supplied by the upstream libraries; this
patch does not replace them. In particular, do not restore russh 0.55 to resolve
dependency conflicts: its allocation paths are covered by RUSTSEC-2026-0153/0154.

The crate is excluded from application workspace membership. Root formatting,
Clippy, workspace tests, loopback SSH authentication, and loopback CredSSP RDP
frame/input/disconnect tests must be rerun when updating or removing this patch.

Original `src/credssp.rs` SHA-256:
`18fa602cd29c98ce53d245c39377ddb5f21aef77ad113cb863138113e3072e11`.

2026-10-22 Linux RDP: add per-hop session ID, opaque X.224 routing and RDSTLS protocol negotiation. Forward encrypted redirect credentials in Client Info without UTF-16 conversion, preserving terminators and checked lengths. Credential/SmartCard Debug output masks passwords, PINs and private keys. Ordinary connections keep their existing security negotiation. See docs/rdp-linux-redirection.md and connection/redirection.rs tests.

2026-11-01: Standard RDP connections may attach the workspace's
`tomato-rdp-security` packet adapter. Advertise the supported 40/56/128-bit
methods, validate server security data, and send Security Exchange before Client
Info. Tag existing Basic Security headers from connector state so the stream
adapter never infers headers from application bytes. Keep TLS/CredSSP separate;
consumers explicitly opt into Java-compatible retries for typed security
negotiation failures. Omit Client Info credentials from debug messages.
The new adapter uses RustCrypto primitives; its framing/key-update logic belongs
to Tomato. Independent synthetic FreeRDP interoperability is required before
claiming legacy protocol support verified.
