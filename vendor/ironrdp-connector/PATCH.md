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
