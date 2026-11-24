"""Build the embedded, on-demand Access adapter. Requires GraalVM 25 Native Image.

No Java runtime is shipped or required on the end user's machine. Downloads are
version and SHA-256 pinned; binaries are built for the current Cargo host only.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
JARS = [
    ("com/healthmarketscience/jackcess/jackcess/4.0.8/jackcess-4.0.8.jar", "bb84e5c7367dedf3a5cea7ad2d37e6874bb688f9003edb92749ef032be25671e"),
    ("org/apache/commons/commons-lang3/3.17.0/commons-lang3-3.17.0.jar", "6ee731df5c8e5a2976a1ca023b6bb320ea8d3539fbe64c8a1d5cb765127c33b4"),
    ("commons-logging/commons-logging/1.3.5/commons-logging-1.3.5.jar", "6d7a744e4027649fbb50895df9497d109f98c766a637062fe8d2eabbb3140ba4"),
    ("com/healthmarketscience/jackcess/jackcess-encrypt/4.0.3/jackcess-encrypt-4.0.3.jar", "d40a7871ac1dc6343cd2e433c3ee484eb59cdff728b7a1e22dcfc8b3f400a18a"),
    ("org/bouncycastle/bcprov-jdk18on/1.86/bcprov-jdk18on-1.86.jar", "2af190b300cbb0b35e248ccf5f4a06b6072030aeb3da7a98ec73abe5b4cb371f"),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target")
    args = parser.parse_args()
    host = next(line[6:] for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
    if args.target and args.target != host:
        raise SystemExit("Native Image must run on the requested Cargo target host")
    target = args.target or host
    directory = ROOT / "target/access-export-build" / target
    directory.mkdir(parents=True, exist_ok=True)
    for relative, expected in JARS:
        path = directory / Path(relative).name
        if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            with urllib.request.urlopen("https://repo.maven.apache.org/maven2/" + relative, timeout=60) as response:
                content = response.read()
            if hashlib.sha256(content).hexdigest() != expected:
                raise SystemExit("Maven artifact checksum mismatch: " + relative)
            path.write_bytes(content)
    java = Path(os.environ["JAVA_HOME"]) / "bin"
    javac = java / ("javac.exe" if os.name == "nt" else "javac")
    native = java / ("native-image.cmd" if os.name == "nt" else "native-image")
    config = directory / "config"
    config.mkdir(exist_ok=True)
    reflection=[
        {"name": "org.apache.commons.logging.impl.NoOpLog", "allPublicConstructors": True},
        {"name": "com.healthmarketscience.jackcess.crypt.util.RC4EngineCompat$Factory", "allDeclaredConstructors": True},
        {"name": "com.healthmarketscience.jackcess.crypt.util.RC4EngineLegacy$Factory", "allDeclaredConstructors": True},
    ]
    with zipfile.ZipFile(directory / "bcprov-jdk18on-1.86.jar") as archive:
        for entry in sorted(archive.namelist()):
            if entry.startswith(("org/bouncycastle/crypto/digests/", "org/bouncycastle/crypto/engines/")) and entry.endswith(("Digest.class", "Engine.class")) and "$" not in entry:
                reflection.append({"name": entry[:-6].replace("/", "."), "allPublicConstructors": True})
    (config / "reflect-config.json").write_text(json.dumps(reflection), encoding="utf-8")
    (config / "resource-config.json").write_text(json.dumps({"resources": {"includes": [{"pattern": "com/healthmarketscience/jackcess/.*"}]}}), encoding="utf-8")
    source = ROOT / "vendor/access-export/TomatoAccess.java"
    subprocess.run([str(javac), "-cp", str(directory / "*"), "-d", str(directory), str(source)], check=True)
    output = ROOT / "vendor/access-export/bin" / target
    output.mkdir(parents=True, exist_ok=True)
    name = "tomato-access.exe" if os.name == "nt" else "tomato-access"
    command = [str(native), "--no-fallback", "-O2", "-march=compatibility", "-J-Xmx6g", "-H:ConfigurationFileDirectories=" + str(config), "-cp", str(directory / "*") + os.pathsep + str(directory), "TomatoAccess", str(output / "tomato-access")]
    subprocess.run(command, check=True)
    (output / "build-info.json").write_text(json.dumps({"target": target, "sourceSha256": hashlib.sha256(source.read_bytes()).hexdigest(), "binarySha256": hashlib.sha256((output / name).read_bytes()).hexdigest(), "jars": JARS}, indent=2), encoding="utf-8")
    print(output / name)


if __name__ == "__main__":
    main()
