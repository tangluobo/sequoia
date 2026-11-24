# Tomato changes

ZGFX token decoding uses a shared 512-byte nine-bit lookup instead of scanning
40 bit-slice prefixes for every token. Byte-aligned raw runs borrow the input
instead of allocating a BitVec. The old token scan and match readers remain
under cfg(test) as an independent oracle; all short prefixes, real corpus
history and truncated error/output behavior are compared. An opt-in alternating
CPU benchmark reports the two paths under the same compiler profile; it does
not measure desktop frame rate.

Progressive tile blocks update coefficient/reference state before visibility
clipping. Only the final visible state in each REGION is reconstructed to RGBA,
removing superseded inverse transforms, temporary tile vectors and a per-REGION
pixel map. Clipped state remains available for a later REGION in the same frame;
repeated difference updates and later reveal are covered by a pixel regression.

RLGR1/3 decoding now reads through a borrowed MSB-first u64 reservoir with
four-byte refills. It avoids allocating or recreating generic bit-slice views
for every entropy symbol. Bounds, short-tail zero-fill and typed error behavior
are preserved. The original decoder is retained only under cfg(test); 2,000
complete/truncated differential cases and unaligned bit-reader sweeps cover
both modes. The encoder and compressed wire representation are unchanged.

Progressive BT.601 reconstruction splits fixed-point coefficients around 65536
before multiplication. Every intermediate fits i32 even for full i16 input,
and signed shifts preserve the original i64 rounding exactly. This permits SIMD
without overflow or changing pixels; a wide-reference regression sweeps the
entire Cb domain against luma/Cr extremes, alongside the existing color vectors.

Pinned codec implementation. Match read-only Java ClearCodec oracle for unwritten/reset VBar slots, variable-height references and clipped short bars. Bound Progressive context reservation and per-payload tile decoding. Allocation tests and 10 independent Java samples cover these changes.

ClearCodec residual runs validate their bounded output slice once and write packed pixels, retaining the existing malformed-run clamp while allowing vector stores.

`decode_into` allows caller-owned scratch reuse. It resets every decoded output
byte, including unwritten pixels, and copies glyph hits into the reusable buffer.
The original allocating decode API delegates to the same implementation.
# Reduce-extrapolate inverse transform

The three fixed Progressive inverse-DWT levels now process eight adjacent
columns and horizontal samples with portable `wide::i16x8`. Signed averages
truncate toward zero, preserving negative odd sums, intermediate i16 wrapping
and the asymmetric extra boundary samples. Scratch stays on the stack; no
additional retained image cache is introduced. An independent copy of the
former scalar inverse transform validates full-range inputs and every one of
4096 coefficient positions; a separately invoked paired CPU benchmark does
not establish physical display FPS. Optional Progressive counters separate
coefficient decoding from pixel reconstruction and report which DWT mode the
actual server uses, without retaining pixels or encoded payloads.
These opt-in reports retain at most 128 lines and flush only on decoder drop;
no diagnostic file writes occur during picture reconstruction.

