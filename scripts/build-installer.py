"""Build the Java-compatible Windows installer without installing it or changing profiles."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parents[1]
VERSION = '3.13'
NSIS_URL = f'https://downloads.sourceforge.net/project/nsis/NSIS%203/{VERSION}/nsis-{VERSION}.zip'
# Pinned against https://nsis-dev.github.io/release-data/versions.json.
NSIS_SHA256 = 'ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8'

def quoted(value):
    value = str(value)
    if any(ord(c) < 32 for c in value):
        raise ValueError('NSIS path contains a control character')
    # File/OutFile/!include are compile-time paths; $$ would name two dollars.
    # Runtime $INSTDIR suffixes are escaped separately in the payload macros.
    return '"' + value.replace('"', '$\\"') + '"'

def payload(stage):
    stage = stage.resolve(strict=True)
    if not stage.is_dir() or not (stage / 'tomato.exe').is_file() or not (stage / 'tomato-cli.exe').is_file():
        raise ValueError('Expected an existing Windows package with both executables')
    files, directories = [], set()
    for directory, folders, names in os.walk(stage, followlinks=False):
        for name in folders + names:
            path = Path(directory) / name
            if path.is_symlink() or path.is_junction() or not path.resolve().is_relative_to(stage):
                raise ValueError('Package contains a link outside its owned payload')
        for name in names:
            path = Path(directory) / name
            if not path.is_file():
                raise ValueError('Only regular package files are supported')
            relative = path.relative_to(stage)
            if relative.name.lower() == 'uninstall.exe':
                raise ValueError('Uninstall.exe is reserved for the generated uninstaller')
            files.append(relative)
            directories.update(p for p in relative.parents if p != Path('.'))
    if len(files) > 10000:
        raise ValueError('Package exceeds 10000 files')
    return sorted(files), sorted(directories, key=lambda p: (-len(p.parts), str(p)))

def manifest(stage):
    files, directories = payload(stage)
    lines = ['!macro InstallPayload']
    previous = None
    for relative in files:
        parent = str(relative.parent).replace('/', '\\')
        if previous != parent:
            literal_parent = parent.replace('$', '$$').replace('"', '$\\"')
            target = '$INSTDIR' + ('\\' + literal_parent if parent != '.' else '')
            lines.append('  SetOutPath "' + target + '"')
            previous = parent
        lines.append('  File ' + quoted(stage / relative))
    lines += ['!macroend', '!macro UninstallPayload']
    lines += ['  Delete "$INSTDIR\\' + str(path).replace('/', '\\').replace('$', '$$').replace('"', '$\\"') + '"' for path in files]
    lines += ['  RMDir "$INSTDIR\\' + str(path).replace('/', '\\').replace('$', '$$').replace('"', '$\\"') + '"' for path in directories]
    lines.append('!macroend')
    return '\n'.join(lines) + '\n'

def compiler(offline):
    override = os.environ.get('TOMATO_MAKENSIS')
    if override:
        return str(Path(override).resolve(strict=True))
    cache = ROOT / 'target' / 'build-tools' / f'nsis-{VERSION}'
    archive = cache.with_name(cache.name + '.zip')
    cache.parent.mkdir(parents=True, exist_ok=True)
    if not archive.exists():
        if offline:
            raise RuntimeError('Verified NSIS cache unavailable in offline mode')
        with urllib.request.urlopen(NSIS_URL, timeout=60) as response:
            content = response.read(4 * 1024 * 1024 + 1)
        if len(content) > 4 * 1024 * 1024 or hashlib.sha256(content).hexdigest() != NSIS_SHA256:
            raise RuntimeError('NSIS publisher checksum mismatch')
        archive.write_bytes(content)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != NSIS_SHA256:
        raise RuntimeError('NSIS cached archive checksum mismatch')
    with tempfile.TemporaryDirectory(dir=cache.parent) as temporary:
        root = Path(temporary)
        with zipfile.ZipFile(archive) as source:
            for item in source.infolist():
                path = root / item.filename
                if not path.resolve().is_relative_to(root.resolve()) or item.file_size > 32 * 1024 * 1024:
                    raise RuntimeError('Invalid NSIS archive entry')
            source.extractall(root)
        tool = root / f'nsis-{VERSION}'
        if not cache.exists():
            shutil.move(tool, cache)
    return str(cache / 'makensis.exe')

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--offline', action='store_true')
    args = parser.parse_args()
    if os.name != 'nt':
        raise RuntimeError('The portable NSIS compiler runs on Windows')
    if not re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+(?:[-+][A-Za-z0-9.-]+)?', args.version):
        raise ValueError('Invalid application version')
    stage = args.stage.resolve(strict=True)
    output = args.output.resolve()
    if output.exists() or output.is_relative_to(stage):
        raise ValueError('Installer output must be new and outside the payload')
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary_root = ROOT / 'target' / 'installer-build'
    temporary_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=temporary_root) as temporary:
        include = Path(temporary) / 'payload.nsh'
        include.write_text(manifest(stage), encoding='utf-8')
        command = [compiler(args.offline), '/V2', '/INPUTCHARSET', 'UTF8',
                   f'/DVERSION={args.version}', f'/DOUTPUT_FILE={output}',
                   f'/DOWNED_FILES={include}', f'/DICON_FILE={ROOT / "apps/tomato/resources/tomato.ico"}',
                   str(ROOT / 'packaging/windows/tomato.nsi')]
        subprocess.run(command, cwd=ROOT, check=True, creationflags=0x08000000)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix(output.suffix + '.sha256').write_text(f'{digest}  {output.name}\n', encoding='utf-8')
    output.with_suffix(output.suffix + '.json').write_text(json.dumps({
        'version': args.version, 'sha256': digest, 'nsisVersion': VERSION,
        'nsisSource': NSIS_URL, 'nsisArchiveSha256': NSIS_SHA256,
        'payloadFiles': len(payload(stage)[0]), 'installed': False,
        'savedConfigurationModified': False,
    }, indent=2), encoding='utf-8')
    print(output)

if __name__ == '__main__':
    main()
