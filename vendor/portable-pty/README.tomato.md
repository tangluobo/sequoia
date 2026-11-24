# Tomato portable-pty patch

Upstream: portable-pty 0.9.0, MIT, copied from the existing Cargo registry cache.
The original license is retained in LICENSE.md.

Only the Windows pseudo-console creation flags change: Tomato renders an independent
terminal cursor, so it must not request PSEUDOCONSOLE_INHERIT_CURSOR. The resize quirk
and Windows input mode flags remain intact. Microsoft documents that cursor inheritance
requires an asynchronous response and can otherwise hang another pseudoconsole operation:
https://learn.microsoft.com/en-us/windows/console/createpseudoconsole

Reproduction: copying a local session from the terminal settings UI can leave an owned
headless conhost with no shell process, while synchronous construction waits for the
cursor handshake. The existing real-PTY creation/drop/backpressure test and UI session-copy
regression run without renderer cursor replies. No Unix behavior is changed.
