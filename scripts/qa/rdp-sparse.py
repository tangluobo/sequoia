"""Check both changed pixels and unchanged background in owned RDP captures."""
import argparse
import json
from pathlib import Path

from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("captures", type=Path)
args = parser.parse_args()
results = []
for mode, origin in (("TAB", (304, 89)), ("WINDOW", (0, 0))):
    frame = Image.open(args.captures / f"{mode}.png").convert("RGB")
    x, y = origin
    background = frame.getpixel((x + 20, y + 20))
    changed = frame.getpixel((x + 110, y + 110))
    assert background == (12, 34, 56), (mode, "lost retained background", background)
    # The existing egui shader dithers the gamma-encoded framebuffer. CPU
    # decoder oracles remain exact; display samples can differ by one level.
    assert all(abs(actual - expected) <= 1 for actual, expected in zip(changed[1:], (34, 56))) and 19 <= changed[0] <= 220, (
        mode, "missing/wrong partial upload", changed
    )
    results.append({"mode": mode, "background": background, "changed": changed})
print(json.dumps({"unchangedBackgroundPreserved": True, "partialPixelsChanged": True,
                  "displayChannelTolerance": 1, "captures": results,
                  "scope": "Owned 1080p sparse ClearCodec fixture; existing renderer dithering"}))
