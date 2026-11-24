# Tomato changes

Expose a consumed compositor-ready signal separately from legacy dirty regions.
The client can publish complete EGFX frames without an extra batching timer.
The existing valid first Progressive preview remains allowed without EndFrame;
this signal neither fabricates acknowledgements nor exposes pending frame tiles.

Native RGBA EGFX composition copies scanlines without channel repacking, retaining pointer save/restore and strict source-length validation. RGBA/BGRA, alpha, offsets, flip and invalid-length regression added; all 46 session tests pass.
