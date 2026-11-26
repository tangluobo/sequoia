"""Bundle and verify the macOS ODBC runtime beside the portable executables and app."""

import argparse
import json
import os
import shutil
import stat
import subprocess
import tempfile
from pathlib import Path


def run(*args: str | Path) -> str:
    return subprocess.check_output(args, text=True).strip()


def imports(path: Path, library: bool = False) -> list[str]:
    loaded = subprocess.check_output(("otool", "-L", path), text=True)
    names = [
        line.strip().split(" (compatibility version ", 1)[0]
        for line in loaded.splitlines()
        if line.startswith("\t")
    ]
    if not names:
        raise ValueError(f"No Mach-O load commands found in {path}")
    return names[1:] if library else names


def system_library(name: str) -> bool:
    return name.startswith(("/usr/lib/", "/System/Library/"))


def formula_for(name: str) -> str:
    if name.startswith(("libodbc.", "libodbcinst.", "libodbccr.")):
        return "unixodbc"
    if name.startswith("libltdl."):
        return "libtool"
    raise ValueError(f"Unexpected non-system macOS dependency {name}")


def codesign(path: Path) -> None:
    run("codesign", "--force", "--sign", "-", path)


def bundle_runtime(stage: Path) -> None:
    app = stage / "Tomato.app"
    llvm_tool = Path(run("brew", "--prefix", "llvm@18")) / "bin/llvm-install-name-tool"
    install_name_tool = llvm_tool if llvm_tool.is_file() else "install_name_tool"
    formulae: dict[str, Path] = {}
    for executables, directory in [
        ([stage / "tomato", stage / "tomato-cli"], stage / "lib"),
        ([app / "Contents/MacOS/tomato"], app / "Contents/Frameworks"),
    ]:
        directory.mkdir()
        work = [(path, False) for path in executables]
        libraries = []
        for path, library in work:
            changes = []
            for dependency in imports(path, library):
                if system_library(dependency):
                    continue
                name = Path(dependency).name
                formula = formula_for(name)
                if formula not in formulae:
                    formulae[formula] = Path(run("brew", "--prefix", formula)).resolve(strict=True)
                destination = directory / name
                if not destination.exists():
                    shutil.copyfile(formulae[formula] / "lib" / name, destination)
                    libraries.append(destination)
                    work.append((destination, True))
                relative = os.path.relpath(destination, path.parent)
                changes.extend(("-change", dependency, f"@loader_path/{relative}"))
            if library:
                changes.extend(("-id", f"@rpath/{path.name}"))
            if changes:
                path.chmod(path.stat().st_mode | stat.S_IWUSR)
                run(install_name_tool, *changes, path)
        for path in libraries + executables:
            codesign(path)

    # Ship the corresponding unmodified upstream source alongside the libraries.
    notices = stage / "docs/third-party/macos-runtime"
    notices.mkdir(parents=True)
    descriptions = []
    for formula, prefix in sorted(formulae.items()):
        info = json.loads(run("brew", "info", "--json=v2", formula))["formulae"][0]
        version = info["versions"]["stable"]
        revision = info["revision"]
        if prefix.name != (f"{version}_{revision}" if revision else version):
            raise ValueError(f"Update {formula} before packaging its source and library")
        run("brew", "fetch", "--build-from-source", formula)
        source = Path(run("brew", "--cache", "--build-from-source", formula))
        archive = notices / source.name.split("--", 1)[-1]
        shutil.copyfile(source, archive)
        descriptions.append(
            f"{formula} {version}\n"
            f"Source and license texts: {archive.name}\n"
            f"Source URL: {info['urls']['stable']['url']}\n"
        )
    notice = "macOS ODBC runtime supplied by Homebrew\n\n" + "\n".join(descriptions)
    (notices / "README.txt").write_text(notice, encoding="utf-8")
    resources = app / "Contents/Resources"
    resources.mkdir(exist_ok=True)
    (resources / "ODBC-SOURCES.txt").write_text(notice, encoding="utf-8")
    codesign(app)


def verify(stage: Path) -> None:
    app = stage / "Tomato.app"
    for executables, directory in [
        ([stage / "tomato", stage / "tomato-cli"], stage / "lib"),
        ([app / "Contents/MacOS/tomato"], app / "Contents/Frameworks"),
    ]:
        libraries = sorted(directory.glob("*.dylib"))
        if not (directory / "libodbc.2.dylib").is_file():
            raise ValueError(f"Missing bundled unixODBC in {directory}")
        for path in executables + libraries:
            run("codesign", "--verify", "--strict", path)
            for dependency in imports(path, path in libraries):
                if system_library(dependency):
                    continue
                prefix, _, relative = dependency.partition("/")
                resolved = (path.parent / relative).resolve()
                if prefix != "@loader_path" or resolved not in libraries:
                    raise ValueError(f"Unbundled dependency in {path}: {dependency}")
    run("codesign", "--verify", "--deep", "--strict", app)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("stage", type=Path)
    parser.add_argument("--verify", action="store_true", help="Verify an extracted package and start its CLI")
    args = parser.parse_args()
    stage = args.stage.resolve(strict=True)
    if not args.verify:
        bundle_runtime(stage)
    verify(stage)
    if args.verify:
        with tempfile.TemporaryDirectory() as directory:
            print(run(stage / "tomato-cli", "--config", Path(directory) / "connections.json", "self-check"))
