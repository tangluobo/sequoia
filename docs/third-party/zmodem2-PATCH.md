Upstream: zmodem2 0.7.3 (MIT OR Apache-2.0), crates.io source.

The sender previously always emitted ZBIN32 for ZFILE, ZDATA and ZEOF.
Negotiate ZBIN (CRC16) when ZRINIT does not advertise CANFC32.
Retain CRC32 for capable peers. This is required for the original Tomato
Java receiver, which advertises no CRC32 capability. Its existing CRC32
serialization is big endian, while standard ZMODEM uses little endian.
sender.rs and file.rs implement encoding negotiation; header.rs adds a wire-encoding builder. Original Java test
oracle sources remain untouched.

The receiver buffer is bounded at 8 KiB instead of 1 KiB, accepting
subpackets emitted by the original Java sender and common SSH senders.
The sender remains bounded at 1 KiB subpackets and honors negotiated pacing.
