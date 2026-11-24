# Tomato adaptation of Risuko eD2k

Source: https://github.com/YueMiyuki/Risuko/tree/95875bfd7478fbb2beafa13e5d178454e361a029/src-tauri/risuko-ed2k
MIT, Copyright 2026-present YueMiyuki (see LICENSE).

This embedded fork keeps the peer, part verification, server and client-only Kad
engines. Tomato supplies its own queue, bounded source parser, progress, profile,
history and target ownership. App-specific HTTP proxy/limiter dependencies were
removed; TCP/UDP connect directly. Additional integration patches are described in
docs/downloader-ed2k.md. No external application or Java runtime is required.
