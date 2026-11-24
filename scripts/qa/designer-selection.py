"""Verify real native grid highlights and row-selector arrows, without editing pixels."""
import json
import sys
from pathlib import Path
from PIL import Image

image = Image.open(sys.argv[1]).convert("RGB")
proof = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8-sig"))
assert image.size == (896, 650), image.size
blue = (53, 146, 203)
if proof["mode"] != "scroll":
    widths = [150, 120, 60, 60, 60, 70, 120, 200]
    for row, mask in enumerate(proof["masks"]):
        x = 16
        for column, width in enumerate(widths):
            selected = image.getpixel((x + width - 7, 94 + row * 30)) == blue
            assert selected == bool(mask & (1 << column)), (proof["mode"], row, column, selected)
            x += width
        arrow = any(
            max(image.getpixel((x, y))) < 80
            for x in range(5, 11)
            for y in range(99 + row * 30, 110 + row * 30)
        )
        assert arrow == bool(mask), (proof["mode"], row, "selector arrow", arrow)
else:
    assert min(proof["visibleFieldRows"]) >= 10
    count = sum(image.getpixel((150, y)) == blue for y in range(92, 390))
    assert count > 180, ("scrolled rows were not visibly highlighted", count)
print(json.dumps({"mode": proof["mode"], "nativePixelsPassed": True}))
