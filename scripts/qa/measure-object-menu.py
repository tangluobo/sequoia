"""Measure a native object-menu framebuffer without depending on viewport-local widget IDs."""
import argparse
import json
from pathlib import Path

from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("image", type=Path)
parser.add_argument("--output", required=True, type=Path)
parser.add_argument("--square-corners", action="store_true")
args = parser.parse_args()
image = Image.open(args.image).convert("RGB")
assert image.size == (896, 650), image.size
pixels = image.load()
ROW_HEIGHT = 23
MENU_HEIGHT = 256
SEPARATOR_OFFSETS = (100, 149, 198, 224)

def is_border(x, y):
    rgb = pixels[x, y]
    return 188 <= rgb[0] <= 192 and max(rgb) - min(rgb) <= 1

runs = []
for y in range(36, 628):
    start = None
    for x in range(897):
        if x < 896 and is_border(x, y):
            if start is None:
                start = x
        elif start is not None:
            if 100 <= x - start <= 300:
                runs.append((start, x - 1, y))
            start = None
components = []
for left_run, right_run, top in runs:
    if not any(abs(a-left_run) <= 1 and abs(b-right_run) <= 1 and y-top == MENU_HEIGHT - 1 for a,b,y in runs):
        continue
    middle = top + MENU_HEIGHT // 2
    left_edges = [x for x in range(max(0,left_run-10),left_run+1) if is_border(x,middle)]
    right_edges = [x for x in range(right_run,min(896,right_run+11)) if is_border(x,middle)]
    if left_edges and right_edges:
        left, right = min(left_edges), max(right_edges)
        components.append((left, top, right-left+1, MENU_HEIGHT))
assert len(components) == 1, components
left, top, width, height = components[0]
assert 150 <= width <= 180, components
right = left + width - 1
bottom = top + height - 1
corners = [(left, top), (right, top), (left, bottom), (right, bottom)]
square = all(is_border(x, y) for x, y in corners)
if args.square_corners:
    assert square, [(x, y, pixels[x, y]) for x, y in corners]
separators = []
for y in range(top + 5, top + height - 5):
    count = sum(pixels[x, y] == (204, 204, 204) for x in range(left + 10, left + width - 10))
    if count >= width - 24:
        separators.append(y - top)
assert len(separators) == 4, separators
assert all(abs(a - b) <= 1 for a, b in zip(separators, SEPARATOR_OFFSETS)), separators
result = {
    "image": str(args.image),
    "rect": [left, top, width, height],
    "separatorOffsets": separators,
    "nativeFramebufferMeasured": True,
    "rowHeight": ROW_HEIGHT,
    "separatorHeight": 3,
    "squareCorners": square,
    "nativePointerTargetVerified": False,
    "fullPixelParity": False,
}
args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
print(json.dumps({"rect": result["rect"], "separatorOffsets": separators}))
