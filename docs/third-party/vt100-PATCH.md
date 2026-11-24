# vt100 local patch

Upstream: vt100 0.15.2, https://github.com/doy/vt100-rust (MIT).

Expose borrowed access to retained historical cells and soft-wrap metadata, the
retained history length, and stable row IDs. A monotonic counter advances when
the bounded history drops its oldest row. Tomato uses these IDs for selections
across scrollback without cloning the screen or keeping a second terminal log.
Cap the combined history and live rows at the viewport height. Deep scrollback
can exceed the viewport height, so the row iterator avoids subtracting the
scroll offset from that height.
ANSI parsing and the existing history bound are unchanged. Replace the deprecated
u8::max_value() spelling with u8::MAX. Sources and upstream license are retained
under vendor/vt100.
