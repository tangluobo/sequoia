"""Verify release packages and stage them under their published names."""

import argparse
import hashlib
import json
from pathlib import Path


def prepare_release(dist: Path, version: str) -> None:
    targets = Path(__file__).resolve().parent.parent / "packaging" / "targets.json"
    platforms = json.loads(targets.read_text(encoding="utf-8"))
    files = []
    checksums = []
    for target, platform in platforms.items():
        suffixes = [".zip", ".tar.gz"]
        if platform.startswith("windows-"):
            suffixes.append("-setup.exe")
        for suffix in suffixes:
            name = f"tomato-{version}-{platform}{suffix}"
            legacy_name = f"tomato-{version}-{target}{suffix}"
            candidates = [
                dist / f"tomato-{platform}" / name,
                dist / f"tomato-{target}" / legacy_name,
            ]
            matches = [path for path in candidates if path.exists() or path.is_symlink()]
            if len(matches) != 1:
                raise ValueError(f"Expected one download for {name}, found {len(matches)}")
            package = matches[0]
            checksum = package.with_name(f"{package.name}.sha256")
            if (
                package.is_symlink()
                or package.parent.is_symlink()
                or checksum.is_symlink()
                or not package.is_file()
                or not checksum.is_file()
            ):
                raise ValueError(f"Missing or invalid package or checksum for {package.name}")
            with package.open("rb") as source:
                digest = hashlib.file_digest(source, "sha256").hexdigest()
            if checksum.read_text(encoding="utf-8").splitlines() != [
                f"{digest}  {package.name}"
            ]:
                raise ValueError(f"Checksum mismatch for {package.name}")
            files.append((package, dist / name))
            checksums.append(f"{digest}  {name}\n")

    for package, destination in files:
        package.replace(destination)
    (dist / "SHA256SUMS.txt").write_text("".join(checksums), encoding="utf-8", newline="\n")
    print(f"Verified {len(files)} packages for v{version}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dist", type=Path)
    parser.add_argument("version")
    args = parser.parse_args()
    try:
        prepare_release(args.dist, args.version)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
