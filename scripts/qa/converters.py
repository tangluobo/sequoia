"""Compare owned converter files with original Java output and capture this GUI."""
from pathlib import Path
import argparse
import copy
import hashlib
import json
import os
import re
import shutil
import subprocess
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest().upper()


def pixels(directory):
    result = {}
    for path in directory.rglob('*.png'):
        if path.stat().st_size:
            with Image.open(path) as image:
                image = image.convert('RGBA')
                result[path.relative_to(directory).as_posix()] = (image.size, image.tobytes())
    return result


def normalized_coco(value):
    value = copy.deepcopy(value)
    value['info'].pop('date_created')
    images = {v['id']: v['file_name'] for v in value['images']}
    categories = {v['id']: v['name'] for v in value['categories']}
    assert len(images) == len(value['images']) and len(categories) == len(value['categories'])
    for image in value['images']:
        image.pop('id')
        image.pop('date_captured')
    for annotation in value['annotations']:
        annotation.pop('id')
        annotation['image_id'] = images[annotation['image_id']]
        annotation['category_id'] = categories[annotation['category_id']]
    for category in value['categories']:
        category.pop('id')
    for field in ['images', 'annotations', 'categories']:
        value[field].sort(key=lambda v: json.dumps(v, sort_keys=True, ensure_ascii=False))
    return value


def owned_program(value, name):
    path = Path(value).resolve(strict=True)
    assert path.name == name and (path.is_relative_to(ROOT / 'target') or path.is_relative_to(ROOT / 'dist'))
    return path


def run(program, arguments, env, out, input_bytes=None):
    startup = None
    flags = 0
    if os.name == 'nt':
        flags = 0x08000000
        startup = subprocess.STARTUPINFO()
        startup.dwFlags = subprocess.STARTF_USESHOWWINDOW
        startup.wShowWindow = 0
    with out.with_suffix('.stdout.log').open('wb') as stdout, out.with_suffix('.stderr.log').open('wb') as stderr:
        subprocess.run([str(program), *arguments], cwd=ROOT, env=env, input=input_bytes,
                       stdout=stdout, stderr=stderr, check=True, timeout=120,
                       creationflags=flags, startupinfo=startup)
    return out.with_suffix('.stdout.log')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--label', required=True)
    parser.add_argument('--cli', required=True)
    parser.add_argument('--gui')
    parser.add_argument('--source', required=True, help='Owned tomato_fixture source directory')
    parser.add_argument('--reference', required=True, help='Completed original Java component probe')
    args = parser.parse_args()
    assert re.fullmatch(r'[a-z0-9-]{1,80}', args.label)
    source = Path(args.source).resolve(strict=True)
    reference = Path(args.reference).resolve(strict=True)
    assert source.is_relative_to(ROOT / 'target') and source.name.startswith('tomato_fixture')
    assert reference.is_relative_to(ROOT / 'target')
    assert json.loads((reference / 'validation.json').read_text(encoding='utf-8'))['passed']
    cli = owned_program(args.cli, 'tomato-cli.exe')
    gui = owned_program(args.gui, 'tomato.exe') if args.gui else None
    output = ROOT / 'target' / 'window-probes' / args.label
    output.mkdir(parents=True, exist_ok=False)
    identities = {'cli': sha(cli), 'gui': sha(gui) if gui else None}
    source_hashes = {str(p): sha(p) for p in source.rglob('*') if p.is_file()}
    env = {k: v for k, v in os.environ.items() if not k.startswith('TOMATO_')}
    profile = output / 'tomato-converter-fixture-cli'
    profile.mkdir()
    env['TOMATO_DATA_DIR'] = str(profile)
    results = []
    coco_reference = json.loads((reference / 'dataset-output/coco_annotations.json').read_text(encoding='utf-8'))
    cases = [('image', 'images', source / 'images'), ('dataset', 'dataset', source / 'voc'), ('image-single', 'images', source / 'images/same.svg')]
    for label, kind, origin in cases:
        directory = output / ('cli-' + label)
        command = {'command': 'convert_tool', 'params': {'request': {'kind': kind, 'source': str(origin), 'directory': str(directory)}}}
        log = run(cli, ['run'], env, output / ('cli-' + label), json.dumps(command, ensure_ascii=False).encode())
        value = json.loads(log.read_text(encoding='utf-8'))
        assert value['type'] == 'conversion' and value['data']['kind'] == kind
        counts = value['data']['counts']
        expected = (6, 3, 3) if kind == 'dataset' else ((1, 1, 0) if label == 'image-single' else (3, 2, 1))
        assert (counts['total'], counts['success'], counts['failed']) == expected
        if kind == 'dataset':
            native = json.loads((directory / 'coco_annotations.json').read_text(encoding='utf-8'))
            assert counts['records'] == 2 and normalized_coco(native) == normalized_coco(coco_reference)
        else:
            assert pixels(directory) == pixels(reference / (label + '-output'))
        results.append({'case': 'cli-' + label, 'counts': counts, 'originalOutputMatched': True})
    captures = []
    if gui:
        for label, kind, origin in cases:
            case = output / ('gui-' + label)
            case.mkdir()
            profile = case / ('tomato-converter-fixture-' + label)
            profile.mkdir()
            copied = profile / 'tomato_fixture_sources'
            shutil.copytree(source, copied)
            config = profile / 'connections.json'
            config.write_text(json.dumps({'connections': [], 'groups': [], 'ownedUnknownField': {'preserve': True}}), encoding='utf-8')
            before = sha(config)
            capture = case / 'capture.png'
            native_env = env | {'TOMATO_DATA_DIR': str(profile), 'TOMATO_UI_CAPTURE': str(capture),
                                'TOMATO_UI_CAPTURE_FRAMES': '2', 'TOMATO_UI_PREVIEW': ('dataset_converter' if kind == 'dataset' else 'image_format_converter'),
                                'TOMATO_UI_PREVIEW_PATH': str(copied / origin.relative_to(source)),
                                'TOMATO_UI_CONVERTER_RUN': 'owned-files', 'WGPU_BACKEND': 'vulkan'}
            run(gui, [], native_env, case / 'application')
            proof = json.loads(capture.with_suffix('.conversion.json').read_text(encoding='utf-8'))
            expected = json.loads((reference / (label + '-completed.json')).read_text(encoding='utf-8'))['status']
            assert proof['status'] == expected and not proof['busy'] and not proof['inlineError']
            assert proof['error'] == (label == 'image') and sha(config) == before
            converted = profile / 'converted'
            if kind == 'dataset':
                native = json.loads((converted / 'coco_annotations.json').read_text(encoding='utf-8'))
                assert normalized_coco(native) == normalized_coco(coco_reference)
            else:
                assert pixels(converted) == pixels(reference / (label + '-output'))
            last = case / 'capture-01.png'
            assert last.is_file()
            captures.append({'case': label, 'path': str(last), 'sha256': sha(last), 'normalExit': True, 'status': expected})
    assert all(sha(Path(p)) == value for p, value in source_hashes.items())
    assert sha(cli) == identities['cli'] and (not gui or sha(gui) == identities['gui'])
    proof = {'passed': True, 'identities': identities, 'cliCases': results, 'nativeCaptures': captures,
             'originalInputUnchanged': True, 'ownedFixturesOnly': True, 'physicalOsInputUsed': False,
             'applicationWideVisualParity': False}
    (output / 'validation.json').write_text(json.dumps(proof, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(proof, ensure_ascii=False), flush=True)


if __name__ == '__main__':
    main()
