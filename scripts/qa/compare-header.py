"""Validate actual title separators and unchanged layout in owned native captures."""
import hashlib
import json
from pathlib import Path
import sys

from PIL import Image, ImageChops

baseline, candidate = map(Path, sys.argv[1:])
validation = json.loads((candidate / "validation.json").read_text(encoding="utf-8-sig"))
assert validation["sharedConfigurationUnchanged"] and not validation["savedHostsContacted"]
proof = []
for entry in validation["captures"]:
    path = Path(entry["path"])
    assert hashlib.sha256(path.read_bytes()).hexdigest().upper() == entry["sha256"]
    old = Image.open(baseline / (entry["state"] + ".png")).convert("RGB")
    new = Image.open(path).convert("RGB")
    assert old.size == new.size
    difference = ImageChops.difference(old, new)
    if entry["state"] == "picker":
        # The picker covers the left part of the title, leaving the right tail
        # exposed. The border must stay beneath the picker and show in the tail.
        assert new.getpixel((500, 52)) == old.getpixel((500, 52))
        for x in range(new.width - 200, new.width - 1):
            assert all(abs(a - b) <= 1 for a, b in zip(new.getpixel((x, 52)), (217, 217, 215)))
        difference.paste((0, 0, 0), (63, 52, new.width - 1, 54))
        assert difference.getbbox() is None, "Picker must change only in the exposed title border"
        proof.append({"state": "picker", "pickerCoversTitleBorder": True, "exposedTitleBorderVisible": True, "unchangedOutsideHeaderBorder": True})
        continue
    for left, right in ((63, 303), (304, new.width - 1)):
        for x in range(left, right):
            pixel = new.getpixel((x, 52))
            assert all(abs(actual - expected) <= 1 for actual, expected in zip(pixel, (217, 217, 215))), (entry["state"], x, pixel)
    assert old.getpixel((500, 52)) == (255, 255, 255), "Baseline must reproduce the missing divider"
    difference.paste((0, 0, 0), (63, 52, new.width - 1, 54))
    animated_scrollbar = None
    if entry["state"] == "rocketmq_narrow" and difference.getbbox() is not None:
        animated_scrollbar = (892, 147, 894, 347)
        for image in (old, new):
            assert all(r == g == b for r, g, b in image.crop(animated_scrollbar).get_flattened_data())
        difference.paste((0, 0, 0), animated_scrollbar)
    assert difference.getbbox() is None, (entry["state"], difference.getbbox())
    proof.append({"state": entry["state"], "fullWidthTitleAndSidebarBordersVisible": True, "unchangedOutsideHeaderBorder": True, "excludedAutoHidingScrollbar": animated_scrollbar})
    if entry["state"] == "kafka":
        new.crop((285, 35, 1199, 107)).save(candidate / "header.png")

result = {"baseline": str(baseline), "borderY": 52, "captures": proof}
(candidate / "header-proof.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
print(json.dumps(result, indent=2))
