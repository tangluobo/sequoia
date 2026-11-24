# Tomato changes

TOMATO_RDP_PIPELINE_TIMING=1 enables bounded per-second counters for ZGFX
decompression, PDU parsing, graphics handling and frame counts. It is off by
default and never logs pixels, credentials, clipboard or input payloads.

Tomato compiles upstream bundled OpenH264, using full-range BT.709 conversion
with runtime-selected SIMD. Compressed AVC packets are bounded to 16 MiB and
decoded RGBA allocation to the existing 16,777,216-pixel desktop limit. The
application supplies V8.1/V8 capabilities when this decoder is explicitly enabled and initializes; the
V10 AVC_DISABLED statement below describes the previous fallback path.

Default connections retain lossless negotiation: a dense 1080p text workload
showed software AVC420 slower than ClearCodec. Complete EGFX frames publish
without the bitmap timer. Consecutive same-surface dirty rectangles coalesce
only if their exact union is rectangular, preserving gaps and cross-surface
paint order with amortized linear metadata processing. Tile grids materialize
one output buffer rather than hundreds. The decompression working buffer retains
at most 4 MiB between PDUs instead of shrinking every bitmap to 16 KiB.

Pinned upstream compositor connected to Tomato DVC. Capabilities follow Java V10 AVC_DISABLED/V8. First valid Progressive output is visible before EndFrame, without premature frame acknowledgement. ZGFX history is lazy; surface/cache budgets remain enforced. See docs/rdp-channels.md.

ClearCodec BGRA→RGBA conversion takes ownership of the decoder buffer and swaps channels in place. It avoids a second full-frame allocation; channel/alpha and allocation reuse are tested.

The client recycles one ClearCodec scratch buffer after compositor/handler
delivery, on both successful and failed decodes. Retained capacity is bounded
to 64 MiB; larger adversarial tiles are not kept. No scratch is preallocated.

`TOMATO_RDP_PIPELINE_TIMING=1` also separates ClearCodec, raw, other codec,
Progressive, EndFrame commit, surface-copy and fill/cache command durations.
Fixed-size frame-gap histograms compare received StartFrame spacing with the
server's timestamps (buckets <10, <20, <40, <80 and <1000 ms; idle gaps omitted).
Counters reset once per report, retain no desktop contents, and are disabled
by default. Reports are retained in a bounded ring (128 lines) and written
through a buffered writer only when the processor is dropped, so periodic
file writes cannot block frame decoding. These are pipeline diagnostics,
not physical display FPS.
Zero or invalid server timestamps are excluded from server interval statistics,
and UTC midnight wraps at 24 hours, following
[MS-RDPEGFX 2.2.2.11](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-rdpegfx/9849fa1a-f896-4abe-9fd4-b7761f56b42c).

Single-destination `SurfaceToSurface` moves within one surface use row-ordered
`copy_within`, eliminating the temporary RGBA snapshot for window dragging and
scrolling. Downward moves copy bottom-up; overlapping horizontal moves use
memmove semantics. Multiple destinations retain the original snapshot so earlier
writes cannot alter later reads. Exhaustive small-surface comparisons cover
overlaps, clipping, zero extents and full-width moves against the old snapshot
path; compositor output tests also verify final dirty pixels.
