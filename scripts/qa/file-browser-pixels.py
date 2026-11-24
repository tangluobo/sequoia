"""Check captured short-directory bodies for the unwanted trailing separator."""
from pathlib import Path
import argparse
import json
from PIL import Image

parser = argparse.ArgumentParser()
parser.add_argument("directory", type=Path)
args = parser.parse_args()
results = []
for mode in ("ICON", "LIST", "COLUMN"):
    path = args.directory / f"{mode}.png"
    with Image.open(path) as image:
        image = image.convert("RGB")
        width, height = image.size
        assert width >= 900 and height >= 600, "expected native workbench window"
        # Exclude the navigation tree, initial file cells, window edge and fixed footer.
        scale = width / 1200  # The owned fixture uses the standard 1200-point window.
        region = (round(600 * scale), round(180 * scale),
                  width - round(24 * scale), height - round(100 * scale))
        body = image.crop(region)
        pixels = body.load()
        lines = []
        white = 0
        for y in range(body.height):
            gray = 0
            for x in range(body.width):
                rgb = pixels[x, y]
                white += min(rgb) >= 250
                gray += max(rgb) - min(rgb) <= 4 and 120 <= max(rgb) < 246
            if gray / body.width > 0.9:
                lines.append(y + region[1])
        fraction = white / (body.width * body.height)
        assert not lines, f"{mode}: unexpected horizontal separator in file body: {lines}"
        assert fraction > 0.95, f"{mode}: short-directory body is not clear white: {fraction}"
        duplicate_root_column = False
        if mode == "COLUMN":
            x = round(504 * scale)  # An unwanted second root column begins here.
            stripe = [image.getpixel((x, y)) for y in range(round(220 * scale), height - round(100 * scale))]
            duplicate_root_column = sum(max(rgb) - min(rgb) <= 4 and 120 <= max(rgb) < 246 for rgb in stripe) > len(stripe) * .9
            assert not duplicate_root_column, "COLUMN: duplicate empty root column"
        results.append({"mode": mode, "size": [width, height], "body": list(region),
                        "whiteFraction": fraction, "unexpectedSeparatorRows": lines,
                        "duplicateRootColumn": duplicate_root_column})
proof = {"captures": results, "scope": "Short-directory body pixels; controls and selection are tested separately"}
(args.directory / "body-pixels.json").write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
print(json.dumps(proof))
