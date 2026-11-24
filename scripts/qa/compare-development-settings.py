"""Compare owned Java/Rust captures. Dimensions passing is not pixel parity."""
import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image, ImageChops, ImageStat

STATES = (
    "project", "sdks", "sdks-selected", "builds", "builds-selected",
    "sdk-add", "tool-add", "tool-edit", "tool-gradle", "tool-type-change",
    "add-directory",
)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compare(java, rust):
    rows = []
    for state in STATES:
        reference, actual = java / f"{state}.png", rust / f"{state}.png"
        with Image.open(reference) as original, Image.open(actual) as rewritten:
            if original.size != rewritten.size:
                raise ValueError(f"{state}: {original.size} != {rewritten.size}")
            diff = ImageChops.difference(original.convert("RGB"), rewritten.convert("RGB"))
            red, green, blue = diff.split()
            changed = ImageChops.lighter(ImageChops.lighter(red, green), blue)
            count = original.width * original.height - changed.histogram()[0]
            rows.append({
                "state": state,
                "size": original.size,
                "javaSha256": digest(reference),
                "rustSha256": digest(actual),
                "differentPixels": count,
                "differentPixelRatio": count / (original.width * original.height),
                "meanAbsoluteChannelDifference": sum(ImageStat.Stat(diff).mean) / 3,
            })
    return {
        "javaDirectory": str(java.resolve()), "rustDirectory": str(rust.resolve()),
        "states": rows, "clientDimensionsMatch": True,
        "fullPixelParity": all(row["differentPixels"] == 0 for row in rows),
        "interactionParityProvenByImages": False,
        "note": "Font rasterization, shadows and focus states contribute to these differences; no tolerance hides them.",
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--proof", type=Path, required=True)
    args = parser.parse_args()
    proof = compare(args.java, args.rust)
    args.proof.write_text(json.dumps(proof, ensure_ascii=False, indent=2), encoding="utf-8")
    print(json.dumps({key: proof[key] for key in (
        "clientDimensionsMatch", "fullPixelParity", "interactionParityProvenByImages"
    )}))
