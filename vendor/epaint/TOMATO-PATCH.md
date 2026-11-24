# Shared font bytes

Based on crates.io epaint 0.36.2, MIT OR Apache-2.0. Original identity is retained
in `.cargo_vcs_info.json`; upstream licenses and source are included.

`text/fonts.rs::blob_from_font_data` shares the existing `Arc<FontData>` as the
font face's byte owner. `FontData` already implements `AsRef<[u8]>`. The previous
conversion cloned the complete `Cow::Owned` font into another allocation for each
font renderer, including each DPI cache. No font data, metrics, rasterization,
shaping, texture sampling or public API changes in this patch.

The UI crate's font asset tests exercise the actual `Fonts::new` constructor,
multiple cache lifetimes and asynchronous font additions. Visual and release
memory probes are documented separately; reference counts alone are not a
whole-process memory measurement.
