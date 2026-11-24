"""Validate owned layout captures and compare tree text against original Java widgets."""
import hashlib
import json
from pathlib import Path
import sys

from PIL import Image, ImageChops


def ink(image):
    rgb = image.convert("RGB")
    mask = Image.eval(ImageChops.lighter(ImageChops.lighter(*rgb.split()[:2]), rgb.split()[2]), lambda p: 255 if p < 140 else 0)
    bounds = mask.getbbox()
    assert bounds, "missing tree label pixels"
    return list(bounds)


reference, candidate = map(Path, sys.argv[1:])
proof = json.loads((candidate / "validation.json").read_text(encoding="utf-8-sig"))
assert proof["sharedConfigurationUnchanged"] and not proof["savedHostsContacted"]
frames = {}
for entry in proof["captures"]:
    path = Path(entry["path"])
    assert hashlib.sha256(path.read_bytes()).hexdigest().upper() == entry["sha256"]
    image = Image.open(path).convert("RGB")
    expected = (900, 900) if entry["state"].endswith("narrow") else (1200, 900)
    assert image.size == expected, (entry["state"], image.size, expected)
    frames[entry["state"]] = image
assert len(frames) == 6
original = Image.open(reference / "kafka-398.png").convert("RGB")
rows = []
for row, name in enumerate(["本地终端", "SSH 生产服务器", "MySQL 数据库", "本地目录"]):
    native = ink(original.crop((20, row * 35, 239, (row + 1) * 35)))
    current = ink(frames["kafka"].crop((104, 54 + row * 35, 298, 89 + row * 35)))
    # The Java fixture omits the graphics. Compare only vertical ink geometry;
    # native fonts and rasterizers may differ by a pixel or two.
    delta = [current[axis] - native[axis] for axis in (1, 3)]
    assert max(map(abs, delta)) <= 2, (name, native, current, delta)
    rows.append({"label": name, "nativeInk": native, "currentInk": current, "verticalDifference": delta})
wide = frames["kafka"].crop((794, 122, 1199, 375))
narrow = frames["kafka_narrow"].crop((600, 122, 899, 375))
sheet = Image.new("RGB", (wide.width + narrow.width, max(wide.height, narrow.height)), "white")
sheet.paste(wide, (0, 0)); sheet.paste(narrow, (wide.width, 0))
sheet.save(candidate / "message-details.png")
result = {"captures": len(frames), "windowSizesMatch": True, "treeTextRows": rows, "maximumVerticalDifference": max(abs(v) for row in rows for v in row["verticalDifference"]), "allApplicationPixelParityVerified": False}
(candidate / "geometry.json").write_text(json.dumps(result, indent=2, ensure_ascii=False), encoding="utf-8")
print(json.dumps(result, ensure_ascii=False))
