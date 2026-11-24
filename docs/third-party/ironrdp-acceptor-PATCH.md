# Tomato test-peer compatibility patch

Upstream: `ironrdp-acceptor` 0.10.0. Both original licenses and the original
manifest are retained. This package is used by the disposable RDP test server.

The only source change propagates the now-fallible `TsRequest::buffer_len()`
result before allocating the outgoing CredSSP buffer. The client connector's
SSPI dependency update is documented in `../ironrdp-connector/PATCH.md`.

Run the loopback CredSSP, raw/RemoteFX image, keyboard, Unicode, mouse and
disconnect tests when changing this patch. No cryptographic algorithm is changed.
