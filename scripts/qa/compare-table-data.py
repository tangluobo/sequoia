"""Check measured open-table geometry/colors; this is not a pixel-parity assertion."""
import argparse
import json
from pathlib import Path

from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("reference", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("--output", required=True, type=Path)
args = parser.parse_args()
reference = Image.open(args.reference).convert("RGB")
candidate = Image.open(args.candidate).convert("RGB")
assert reference.size == candidate.size == (896, 650)
checks = []
for name, point in [
    ("full_width_action_bar", (600, 20)),
    ("header_background", (25, 40)),
    ("empty_filler", (600, 45)),
    ("first_row_white", (100, 78)),
    ("second_row_white", (100, 106)),
    ("bottom_actions", (500, 610)),
    ("sql_status_background", (600, 648)),
]:
    expected, actual = reference.getpixel(point), candidate.getpixel(point)
    assert expected == actual, (name, expected, actual)
    checks.append({"name": name, "point": point, "rgb": actual})
# The 28px row pitch and UTF-16 header-length sizing must remain invariant.
for x in [87, 147, 219, 279]:
    for y in [85, 113, 141]:
        assert candidate.getpixel((x, y)) == reference.getpixel((x, y)), (x, y)
for y in [92, 120, 148, 176]:
    assert candidate.getpixel((100, y)) == reference.getpixel((100, y)), y
selection_boxes = []
for path in [args.reference.with_name("selected.png"), args.candidate.with_name("selected.png")]:
    selected = Image.open(path).convert("RGB")
    points = [(x, y) for y in range(80, 125) for x in range(300)
              if selected.getpixel((x, y)) == (53, 146, 203)]
    assert points, ("missing selected cell", path)
    selection_boxes.append([min(x for x, y in points), min(y for x, y in points),
                            max(x for x, y in points), max(y for x, y in points)])
assert selection_boxes[0] == selection_boxes[1], selection_boxes
args.output.write_text(json.dumps({
    "reference": str(args.reference), "candidate": str(args.candidate),
    "checks": checks, "rowHeight": 28, "selectorWidth": 15,
    "selectedCellBounds": selection_boxes[1], "selectedCellRgb": [53, 146, 203],
    "allStatesPixelIdentical": False,
    "scope": "Initial open-table geometry/colors, original default 10px font; excludes font rasterization and other dialogs",
}, ensure_ascii=False, indent=2), encoding="utf-8")
print(args.output)
