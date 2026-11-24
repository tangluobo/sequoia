"""Check the actual upload child framebuffer: background, bar height and text spacing."""
import json
import sys
from pathlib import Path

from PIL import Image

folder = Path(sys.argv[1])
image = Image.open(folder / "s3-list-progress.png").convert("RGB")
assert image.size == (484, 151), image.size
for point in [(0, 0), (483, 150), (2, 70), (240, 140)]:
    assert image.getpixel(point) == (255, 255, 255), ("background", point, image.getpixel(point))
green = [
    (x, y)
    for y in range(image.height)
    for x in range(image.width)
    if (lambda rgb: rgb[0] < 30 and rgb[1] > 150 and 40 < rgb[2] < 160)(image.getpixel((x, y)))
]
assert green
bar = (min(x for x, _ in green), min(y for _, y in green), max(x for x, _ in green) + 1, max(y for _, y in green) + 1)
assert bar[0] == 15 and bar[1] == 67 and bar[2] <= 469 and bar[3] == 79, ("bar bounds", bar)
counts = {}
for name, bounds in [("title", (15, 15, 469, 32)), ("currentFile", (15, 42, 469, 57)), ("detail", (15, 89, 469, 102))]:
    patch = image.crop(bounds)
    counts[name] = sum(max(rgb) < 150 for rgb in patch.getdata())
    assert counts[name] > 50, ("missing visible text", name, counts[name])
proof = {"status": "passed", "size": image.size, "barBounds": bar, "visibleTextPixels": counts, "background": "white", "scope": "upload child framebuffer; no physical OS drag claim"}
(folder / "pixels.json").write_text(json.dumps(proof, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(proof, ensure_ascii=False))
