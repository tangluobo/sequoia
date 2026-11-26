"""Start and capture the packaged macOS desktop with both OpenGL entry points."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile

stage = Path(sys.argv[1])
with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    for mode, executable, options in [
        ("fallback", stage / "tomato", {"WGPU_BACKEND": "dx12"}),
        ("opengl", stage / "Tomato.app/Contents/MacOS/tomato", {"TOMATO_RENDERER": "opengl"}),
    ]:
        output = root / f"{mode}.png"
        env = {
            **os.environ,
            "TOMATO_DATA_DIR": str(root / mode),
            "TOMATO_UI_CAPTURE": str(output),
            "TOMATO_RENDER_DIAGNOSTICS": "1",
        }
        env.pop("TOMATO_RENDERER", None)
        env.pop("WGPU_BACKEND", None)
        env.update(options)
        result = subprocess.run([executable], env=env, text=True, capture_output=True, timeout=65)
        print(result.stderr)
        if result.returncode or not output.is_file():
            raise SystemExit(f"{mode} did not render and exit successfully")
        if mode == "fallback" and "retrying with OpenGL" not in result.stderr:
            raise SystemExit("The failed wgpu startup did not exercise the OpenGL recovery path")
        if not output.read_bytes().startswith(b"\x89PNG\r\n\x1a\n"):
            raise SystemExit(f"{mode} failed to write a PNG")
        print(f"Rendered and closed the native macOS window in {mode} mode")
