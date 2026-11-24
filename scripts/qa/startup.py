"""Measure owned GUI processes from launch to their first visible, painted window.

EnumWindows is read-only and restricted to the PID just launched. eframe keeps
the root hidden until after painting. This measures native window readiness,
not screenshot encoding (the capture fixture intentionally waits 700 ms).
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import threading
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest().upper()


def visible_window(pid):
    user = ctypes.windll.user32
    found = []
    callback_type = ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    user.EnumWindows.argtypes = [callback_type, wintypes.LPARAM]
    user.EnumWindows.restype = wintypes.BOOL
    user.GetWindowThreadProcessId.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.DWORD)]
    user.IsWindowVisible.argtypes = [wintypes.HWND]
    user.IsWindowVisible.restype = wintypes.BOOL
    user.GetClientRect.argtypes = [wintypes.HWND, ctypes.POINTER(wintypes.RECT)]

    @callback_type
    def visit(hwnd, _):
        owner = wintypes.DWORD()
        user.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user.IsWindowVisible(hwnd):
            rect = wintypes.RECT()
            user.GetClientRect(hwnd, ctypes.byref(rect))
            if rect.right >= 800 and rect.bottom >= 600:
                found.append([rect.right, rect.bottom])
        return True

    user.EnumWindows(visit, 0)
    return found[0] if found else None


def run(exe, directory, profile, backend, diagnostics):
    directory.mkdir()
    config = directory / "profile"
    config.mkdir()
    if profile:
        # Copy only connections/preferences. Schedulers have no jobs in this
        # isolated profile, and opening a connection requires a human action.
        for name in ("connections.json", "global.json"):
            source = profile / name
            if source.exists():
                shutil.copyfile(source, config / name)
    before = {p.name: digest(p) for p in config.iterdir()}
    env = {k: v for k, v in os.environ.items()
           if not k.startswith("TOMATO_") and k != "WGPU_BACKEND"}
    env.update(TOMATO_DATA_DIR=str(config), TOMATO_UI_CAPTURE=str(directory / "frame.png"),
               TOMATO_CAPTURE_TRACE="1")
    if diagnostics:
        env["TOMATO_RENDER_DIAGNOSTICS"] = "1"
    if backend != "auto":
        env["WGPU_BACKEND"] = backend
    timeline = []
    start = time.perf_counter()
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = 0
    proc = subprocess.Popen([str(exe)], env=env, stderr=subprocess.PIPE,
                            stdout=subprocess.DEVNULL, startupinfo=startup)

    def logs():
        for line in iter(proc.stderr.readline, b""):
            timeline.append({"ms": round((time.perf_counter() - start) * 1000, 3),
                             "message": line.decode("utf-8", errors="replace").rstrip()})

    reader = threading.Thread(target=logs)
    reader.start()
    ready = None
    size = None
    try:
        while proc.poll() is None:
            if ready is None:
                size = visible_window(proc.pid)
                if size:
                    ready = round((time.perf_counter() - start) * 1000, 3)
            if time.perf_counter() - start > 45:
                raise RuntimeError("Owned GUI did not exit normally within 45 s")
            time.sleep(0.002)
        reader.join(5)
        if proc.returncode or ready is None or not (directory / "frame.png").is_file():
            raise RuntimeError(f"GUI failed: code={proc.returncode}, ready={ready}, {timeline[-8:]}")
    finally:
        if proc.poll() is None:
            proc.kill()  # only the exact subprocess owned by this fixture
            proc.wait()
        reader.join(5)
        (directory / "timeline.json").write_text(json.dumps(timeline, indent=2), encoding="utf-8")
    assert all(digest(config / name) == value for name, value in before.items()), "Config changed"
    return {"firstPaintedVisibleMs": ready, "size": size, "normalExit": proc.returncode == 0,
            "profileUnchanged": True, "diagnostics": diagnostics}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--old", type=Path, required=True)
    parser.add_argument("--new", type=Path)
    parser.add_argument("--label", required=True)
    parser.add_argument("--runs", type=int, default=3)
    parser.add_argument("--profile", type=Path, default=Path.home() / ".tomato")
    parser.add_argument("--backends", nargs="+", default=["auto", "vulkan"])
    parser.add_argument("--diagnostics", action="store_true")
    args = parser.parse_args()
    if not re.fullmatch(r"[a-z0-9-]+", args.label) or not 1 <= args.runs <= 10:
        parser.error("Use a fresh lowercase probe label and 1..10 runs")
    root = Path(__file__).resolve().parents[2]
    probe = root / "target/window-probes" / args.label
    probe.mkdir()
    versions = {"old": args.old.resolve()}
    if args.new:
        versions["new"] = args.new.resolve()
    copies = {}
    for version, source in versions.items():
        folder = probe / version
        folder.mkdir()
        target = folder / "tomato.exe"
        shutil.copyfile(source, target)
        copies[version] = {"source": str(source), "sha256": digest(source), "executable": str(target)}
        assert digest(target) == copies[version]["sha256"]
    results = []
    warmups = []
    for backend in args.backends:
        for profile in ("empty", "legacy"):
            for version, copy in copies.items():
                warmup = run(Path(copy["executable"]), probe / version / f"{backend}-{profile}-warmup",
                             args.profile if profile == "legacy" else None, backend, args.diagnostics)
                warmup.update(version=version, backend=backend, profile=profile)
                warmups.append(warmup)
                print(json.dumps({"warmup": warmup}), flush=True)
            for iteration in range(args.runs):
                # Alternate order to reduce filesystem/driver cache bias.
                order = list(copies) if iteration % 2 == 0 else list(reversed(copies))
                for version in order:
                    name = f"{backend}-{profile}-{iteration + 1}"
                    directory = probe / version / name
                    result = run(Path(copies[version]["executable"]), directory,
                                 args.profile if profile == "legacy" else None,
                                 backend, args.diagnostics)
                    result.update(version=version, backend=backend, profile=profile, run=iteration + 1)
                    results.append(result)
                    print(json.dumps(result), flush=True)
    for copy in copies.values():
        assert digest(Path(copy["source"])) == copy["sha256"]
        assert digest(Path(copy["executable"])) == copy["sha256"]
    report = {"results": results, "warmups": warmups, "executables": copies, "savedHostsContacted": False,
              "osInputInjected": False, "scope": "warm filesystem/driver caches; owned native root visibility after eframe first paint; no screenshot delay included; not cold-boot or physical scanout"}
    (probe / "validation.json").write_text(json.dumps(report, indent=2), encoding="utf-8")


if __name__ == "__main__":
    main()
