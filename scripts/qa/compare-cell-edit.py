"""Compare the real original in-cell editor with the owned Rust viewport."""
import hashlib
import json
from pathlib import Path
import sys
from PIL import Image

reference, candidate = map(Path, sys.argv[1:])
events_path = reference / 'editing.png.events.json'
events = json.loads(events_path.read_text(encoding='utf-8'))
assert events['firstClickEdits'] is False
assert events['hoverStartsEdit'] is False and events['f2Edits']
assert events['selectedClickEdits'] is True and events['inlineTextField']
assert events['nativeWindows'] == 1
assert events['enterValue'] == 'committed' and events['enterClosesEditor']
assert events['escapeValue'] == 'committed' and events['escapeClosesEditor']
assert events['blurValue'] == 'blurred' and not events['networkRequested']
validation = json.loads((candidate / 'validation.json').read_text(encoding='utf-8-sig'))
assert validation['sharedConfigurationUnchanged'] and not validation['savedHostsContacted']
for entry in validation['captures']:
    path = Path(entry['path'])
    assert hashlib.sha256(path.read_bytes()).hexdigest().upper() == entry['sha256']
    assert Image.open(path).size == (896, 650)

native = Image.open(reference / 'editing.png').convert('RGB')
actual = Image.open(candidate / 'editing.png').convert('RGB')
blue, selection = (53, 146, 203), (0, 150, 201)

def close(actual, expected):
    return all(abs(a-b) <= 1 for a, b in zip(actual, expected))

def bounds(image, color):
    points = [(x, y) for y in range(90, 124) for x in range(250, 425)
              if image.getpixel((x, y)) == color]
    assert points, ('missing in-cell color', color)
    return [min(x for x, y in points), min(y for x, y in points),
            max(x for x, y in points), max(y for x, y in points)]

border = bounds(native, blue)
assert border == bounds(actual, blue) == [256, 93, 415, 120]
left, top, right, bottom = border
for image in (native, actual):
    for x in range(left, right + 1):
        for y in (top, top + 1, bottom - 1, bottom):
            assert close(image.getpixel((x, y)), blue), ('horizontal editor border', x, y)
    for y in range(top, bottom + 1):
        for x in (left, right):
            assert close(image.getpixel((x, y)), blue), ('vertical editor border', x, y)
        # Java's fractional TextField background partially covers its inner
        # vertical border pixel. Preserve the real reference raster difference.
        for x in (left + 1, right - 1):
            pixel = image.getpixel((x, y))
            alpha = [(channel-base)/(255-base) for channel, base in zip(pixel, blue)]
            assert min(alpha) >= -.02 and max(alpha) <= .2 and max(alpha)-min(alpha) < .03, (
                'inner vertical border must retain the blue edge', x, y, pixel)
    for point in ((left + 3, top + 3), (right - 3, top + 3), (right - 3, bottom - 3)):
        assert close(image.getpixel(point), (255, 255, 255)), ('editor background', point)
native_selection, actual_selection = bounds(native, selection), bounds(actual, selection)
assert all(abs(a-b) <= 2 for a, b in zip(native_selection, actual_selection)), (
    'selection and text must keep the original inset and vertical center',
    native_selection, actual_selection)
proof = {
    'original': str(reference), 'candidate': str(candidate),
    'originalEvents': events, 'originalEventSha256': hashlib.sha256(events_path.read_bytes()).hexdigest(),
    'guiSha256': validation['guiSha256'], 'cellBorder': border,
    'borderPixels': 2, 'editorBackground': 'white',
    'originalSelectionBounds': native_selection, 'candidateSelectionBounds': actual_selection,
    'allBorderPixelsIdentical': False,
    'allGlyphPixelsIdentical': False,
    'scope': 'Real original control events, editor geometry, outer edges and colors within one channel level for GPU quantization. Java inner vertical edges have a partial white background blend; font selection extents differ by up to 2px. Rust pointer, keyboard and staging behavior are checked by workspace tests.',
}
(candidate / 'interaction-proof.json').write_text(json.dumps(proof, indent=2, ensure_ascii=False), encoding='utf-8')
print(json.dumps(proof, indent=2, ensure_ascii=False))
