# Local rendering adapter changes

Source: egui-wgpu 0.36.2, copied from the existing Cargo registry package.
Upstream metadata, copyright headers and MIT OR Apache-2.0 license designation are preserved.

The Windows device requests dual-source blending only when supported. An optional font-atlas pipeline blends RGB coverage with the actual destination independently for each channel. Normal image textures and GPU callbacks keep the original pipeline. Unsupported devices and sRGB framebuffers retain scalar-alpha fonts.

Font atlas format negotiation is explicit in Tomato. epaint keeps one RGBA texel per output pixel; only the temporary small glyph raster is sampled at three horizontal positions.
