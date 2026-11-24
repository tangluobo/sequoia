"""Check the visible time-field ink against the original owned Java snapshot."""
import argparse
import json
from pathlib import Path

from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("before", type=Path)
parser.add_argument("after", type=Path)
parser.add_argument("java", type=Path)
parser.add_argument("output", type=Path)
args = parser.parse_args()
images = {name: Image.open(path).convert("RGB") for name, path in
          [("before", args.before), ("after", args.after), ("java", args.java)]}
assert {image.size for image in images.values()} == {(260, 262)}
fields = []
for index in range(3):
    left = 27 + 76 * index
    bounds = (left + 3, 192, left + 29, 218)
    result = {"field": ["hour", "minute", "second"][index], "fieldCenterY": 205}
    for name, image in images.items():
        crop = image.crop(bounds)
        pixels = [(x, y) for y in range(crop.height) for x in range(crop.width)
                  if max(crop.getpixel((x, y))) < 160]
        assert pixels, (name, index, "missing time digits")
        y0 = min(y for _, y in pixels) + bounds[1]
        y1 = max(y for _, y in pixels) + bounds[1] + 1
        result[name] = {"inkTop": y0, "inkBottom": y1,
                        "inkCenterY": (y0 + y1) / 2,
                        "centerOffsetPx": (y0 + y1) / 2 - 205}
    assert result["before"]["centerOffsetPx"] < -4, result
    assert abs(result["after"]["centerOffsetPx"]) <= 2, result
    assert abs(result["after"]["inkCenterY"] - result["java"]["inkCenterY"]) <= 2, result
    fields.append(result)
args.output.write_text(json.dumps({"fields": fields, "originalGeometryPreserved": True,
                                 "timeDigitsVisuallyCentered": True},
                                ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(fields, ensure_ascii=False))
