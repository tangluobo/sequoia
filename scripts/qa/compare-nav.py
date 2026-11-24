"""Compare owned native captures after replacing only navigation SVG assets."""
import hashlib
import json
from pathlib import Path
import sys

from PIL import Image, ImageChops


baseline, candidate = map(Path, sys.argv[1:])
validation = json.loads((candidate / "validation.json").read_text(encoding="utf-8-sig"))
assert validation["sharedConfigurationUnchanged"]
assert not validation["savedHostsContacted"]
boxes = [(18, int(y - 14), 46, int(y + 14)) for y in (41, 93.5, 146, 196, 246)]
proof = []
for entry in validation["captures"]:
    path = Path(entry["path"])
    assert hashlib.sha256(path.read_bytes()).hexdigest().upper() == entry["sha256"]
    old = Image.open(baseline / (entry["state"] + ".png")).convert("RGB")
    new = Image.open(path).convert("RGB")
    assert old.size == new.size
    difference = ImageChops.difference(old, new)
    if entry["state"] == "picker":
        # This opaque picker covers the navigation rail completely.
        assert difference.getbbox() is None
        proof.append({"state": entry["state"], "navigationCoveredByPicker": True, "wholeFrameUnchanged": True})
        continue
    for box in boxes:
        assert ImageChops.difference(old.crop(box), new.crop(box)).getbbox()
        assert sum(max(pixel) - min(pixel) > 40 for pixel in new.crop(box).get_flattened_data()) > 40
        difference.paste((0, 0, 0), box)
    animated_scrollbar = None
    if entry["state"] == "rocketmq_narrow" and difference.getbbox() is not None:
        # The unchanged egui InfoScroll has an auto-hiding thumb. The historical
        # capture includes it; a fresh capture may hide it after the pointer leaves.
        # Exclude only its two-pixel track, never message text or layout bounds.
        animated_scrollbar = (892, 147, 894, 347)
        for image in (old, new):
            assert all(r == g == b for r, g, b in image.crop(animated_scrollbar).get_flattened_data())
        difference.paste((0, 0, 0), animated_scrollbar)
    assert difference.getbbox() is None, (entry["state"], difference.getbbox())
    proof.append({
        "state": entry["state"],
        "unchangedOutsideIconsAndListedAnimation": True,
        "excludedAutoHidingScrollbar": animated_scrollbar,
    })
    if entry["state"] == "kafka":
        new.crop((1, 20, 62, 271)).save(candidate / "navigation.png")

result = {"baseline": str(baseline), "iconBoxes": boxes, "captures": proof}
(candidate / "navigation-proof.json").write_text(json.dumps(result, indent=2), encoding="utf-8")
print(json.dumps(result, indent=2))
