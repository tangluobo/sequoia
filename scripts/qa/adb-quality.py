"""Compare the owned 2560x1440 fine-text fixture at its 1200x900 GUI layout.

Reference is the decoded fixture's RGBA, not the uncompressed source. This checks
display sampling only, not Android encoder fidelity or end-to-end frame latency.
"""
from pathlib import Path
from math import floor
from PIL import Image
import argparse
import json

parser = argparse.ArgumentParser()
parser.add_argument("original", type=Path)
parser.add_argument("candidate", type=Path)
parser.add_argument("--reference", type=Path, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
reference = Image.frombytes("RGBA", (2560, 1440), args.reference.read_bytes())
pixels = reference.load()
images = []
proofs = []
for folder in (args.original, args.candidate):
    proof = json.loads((folder / "validation.json").read_text(encoding="utf-8-sig"))
    assert proof["staticQualityOnly"] and proof["sharedConfigurationUnchanged"]
    assert not proof["savedHostsContacted"]
    image = Image.open(folder / "adb.png").convert("RGB")
    assert image.size == (1200, 900)
    # Actual GPU viewport, rounded from the same aspect-fit layout in both builds.
    for x, y in ((304, 259), (1198, 259), (304, 761), (1198, 761)):
        r, g, b = image.getpixel((x, y))
        assert r < 40 and g > 130 and b > 170, (folder, x, y, (r, g, b))
    images.append(image)
    proofs.append(proof["guiSha256"])

errors = [0.0, 0.0]
count = 0
for y in range(283, 700, 3):
    for x in range(324, 885, 3):
        fx = (x + 0.5 - 304) * 2560 / 895 - 0.5
        fy = (y + 0.5 - 259) * 1440 / 503 - 0.5
        ix, iy = floor(fx), floor(fy)
        dx, dy = fx - ix, fy - iy
        value = (
            (pixels[ix, iy][0] * (1 - dx) + pixels[ix + 1, iy][0] * dx) * (1 - dy)
            + (pixels[ix, iy + 1][0] * (1 - dx) + pixels[ix + 1, iy + 1][0] * dx) * dy
        )
        for index, image in enumerate(images):
            errors[index] += abs(image.getpixel((x, y))[0] - value)
        count += 1
errors = [error / count for error in errors]
assert errors[1] < 2.0 and errors[1] < errors[0] * 0.5, errors
result = {
    "originalGuiSha256": proofs[0],
    "candidateGuiSha256": proofs[1],
    "reference": str(args.reference),
    "sampleCount": count,
    "originalMeanAbsoluteError": errors[0],
    "candidateMeanAbsoluteError": errors[1],
    "actualGpuShaderRendered": True,
    "scope": "Fine-text grayscale ROI; red-channel error against decoded-frame bilinear sampling. Not general image quality, Android fidelity, FPS or physical scanout.",
}
args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
print(json.dumps(result))
