"""Windows release CLI comparison on a bounded, owned multi-file VOC dataset."""
from pathlib import Path
import argparse
import ctypes
from ctypes import wintypes
import gc
import hashlib
import json
import os
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest().upper()


class Memory(ctypes.Structure):
    _fields_ = [('cb', wintypes.DWORD), ('PageFaultCount', wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in ['PeakWorkingSetSize', 'WorkingSetSize',
        'QuotaPeakPagedPoolUsage', 'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage',
        'QuotaNonPagedPoolUsage', 'PagefileUsage', 'PeakPagefileUsage', 'PrivateUsage']]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--old', required=True)
    parser.add_argument('--new', required=True)
    parser.add_argument('--label', required=True)
    parser.add_argument('--files', type=int, default=20)
    parser.add_argument('--objects', type=int, default=5000)
    args = parser.parse_args()
    assert os.name == 'nt' and re.fullmatch(r'[a-z0-9-]{1,80}', args.label)
    assert 1 <= args.files <= 100 and 1 <= args.objects <= 10000 and args.files * args.objects <= 200000
    programs = {name: Path(value).resolve(strict=True) for name, value in [('old', args.old), ('new', args.new)]}
    assert all(p.name == 'tomato-cli.exe' and (p.is_relative_to(ROOT / 'target') or p.is_relative_to(ROOT / 'dist')) for p in programs.values())
    identities = {name: sha(path) for name, path in programs.items()}
    output = ROOT / 'target' / 'window-probes' / args.label
    output.mkdir(parents=True, exist_ok=False)
    source = output / 'tomato_converter_memory_sources'
    source.mkdir()
    item = '<object><name>cat</name><bndbox><xmin>10</xmin><ymin>20</ymin><xmax>40</xmax><ymax>60</ymax></bndbox></object>'
    for index in range(args.files):
        text = f'<annotation><filename>image-{index}.jpg</filename><size><width>100</width><height>80</height></size>' + item * args.objects + '</annotation>'
        (source / f'image-{index:04}.xml').write_text(text, encoding='utf-8')
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    memory_api = ctypes.WinDLL('psapi', use_last_error=True).GetProcessMemoryInfo
    memory_api.argtypes = [wintypes.HANDLE, ctypes.POINTER(Memory), wintypes.DWORD]
    memory_api.restype = wintypes.BOOL
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.CloseHandle.restype = wintypes.BOOL
    samples = []
    for index, version in enumerate(['old', 'new', 'old', 'new']):
        profile = output / f'profile-{index}-{version}'
        profile.mkdir()
        destination = profile / 'coco.json'
        env = {k: v for k, v in os.environ.items() if not k.startswith('TOMATO_')}
        env['TOMATO_DATA_DIR'] = str(profile)
        payload = json.dumps({'command': 'convert_dataset', 'params': {'source': str(source), 'destination': str(destination)}}).encode()
        handle = None
        process = None
        start = time.monotonic()
        observations = []
        with (profile / 'stdout.log').open('wb') as out, (profile / 'stderr.log').open('wb') as err:
            try:
                process = subprocess.Popen([str(programs[version]), 'run'], cwd=ROOT, env=env,
                                           stdin=subprocess.PIPE, stdout=out, stderr=err, creationflags=0x08000000)
                handle = kernel.OpenProcess(0x0400 | 0x0010, False, process.pid)
                assert handle, 'Could not observe own CLI memory'
                process.stdin.write(payload)
                process.stdin.close()
                while process.poll() is None:
                    value = Memory()
                    value.cb = ctypes.sizeof(value)
                    if memory_api(handle, ctypes.byref(value), value.cb):
                        observations.append((value.PeakWorkingSetSize, value.PrivateUsage, value.PeakPagefileUsage))
                    if time.monotonic() - start > 180:
                        raise TimeoutError('Owned CLI conversion exceeded benchmark limit')
                    time.sleep(.01)
                assert process.returncode == 0 and observations
            finally:
                if process is not None and process.poll() is None:
                    process.kill()
                    process.wait(timeout=10)
                if handle:
                    kernel.CloseHandle(handle)
        seconds = round(time.monotonic() - start, 3)
        result = json.loads((profile / 'stdout.log').read_text(encoding='utf-8'))
        assert result['type'] == 'text'
        value = json.loads(destination.read_text(encoding='utf-8'))
        assert len(value['images']) == args.files and len(value['annotations']) == args.files * args.objects
        assert all(v['bbox'] == [10, 20, 30, 40] and v['area'] == 1200 for v in value['annotations'])
        del value
        gc.collect()
        result = {'version': version, 'round': index, 'seconds': seconds, 'samples': len(observations),
                  'peakWorkingSetMiB': round(max(v[0] for v in observations) / 1048576, 2),
                  'maxSampledPrivateCommitMiB': round(max(v[1] for v in observations) / 1048576, 2),
                  'peakCommitChargeMiB': round(max(v[2] for v in observations) / 1048576, 2),
                  'outputBytes': destination.stat().st_size, 'actualImages': args.files, 'actualAnnotations': args.files * args.objects}
        samples.append(result)
        print(json.dumps(result), flush=True)
    assert all(sha(programs[name]) == identities[name] for name in programs)
    proof = {'passed': True, 'identities': identities, 'samples': samples, 'ownedFilesOnly': True,
             'savedConnectionsUsed': False, 'physicalOsInputUsed': False,
             'scope': 'Own CLI spawn to successful exit on same valid dataset, alternating twice. Working-set peak and sampled private commit are separate; this is not whole-application or active-session memory.'}
    (output / 'validation.json').write_text(json.dumps(proof, indent=2), encoding='utf-8')


if __name__ == '__main__':
    main()
