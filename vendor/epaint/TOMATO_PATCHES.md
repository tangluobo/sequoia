# Tomato changes to epaint 0.36.2

The existing font loader shares owned font bytes between face instances and DPI
caches. The upstream MIT / Apache-2.0 license continues to apply.

`LayoutJob::align_on_baseline` is an opt-in document layout mode. It measures
each row's largest face ascent and descent, places all glyph origins on a common
baseline before tessellation, and includes the mode in the galley cache key.
Fallback faces, syntax colors, selections, links, decorations and wrapping keep
their ordinary geometry. Empty rows retain their measured height. The default is
false, including when deserializing older jobs, so controls, terminals and other
existing layouts keep their current behavior. Tomato opts in for Markdown source,
preview spans and highlighted code, matching JavaFX TextFlow's baseline alignment.
The change adds no font copies, glyph cache or extra per-row allocation.
