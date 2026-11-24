"""Compare unchanged row boundaries in actual native designer framebuffers."""
import argparse
import json
from pathlib import Path
from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("baseline", type=Path)
parser.add_argument("states", nargs="+", type=Path)
parser.add_argument("--proof", required=True, type=Path)
args = parser.parse_args()


def boundaries(path):
    with Image.open(path) as image:
        image = image.convert("RGB")
        if image.size != (896, 650):
            raise ValueError(f"Wrong native viewport: {path}")
        # The selector column is outside every editor and popup. Its unscaled
        # Java-style bottom borders expose shifts of any subsequent row.
        return [y for y in range(90, 211)
                if image.getpixel((8, y)) == (190, 190, 188)]


expected = boundaries(args.baseline)
if len(expected) != 3 or any(b - a != 30 for a, b in zip(expected, expected[1:])):
    raise ValueError(f"Unexpected static grid geometry: {expected}")
results = []
for path in args.states:
    actual = boundaries(path)
    if actual != expected:
        raise ValueError(f"Row boundaries moved in {path.name}: {actual} != {expected}")
    results.append(dict(image=str(path), boundaries=actual, unchanged=True))
proof = dict(baseline=str(args.baseline), rowHeight=30, boundaries=expected,
             states=results, allRowsUnchanged=True, fullJavaPixelParityClaimed=False)
args.proof.write_text(json.dumps(proof, ensure_ascii=False, indent=2), encoding="utf-8")
print(json.dumps(proof, ensure_ascii=False))
