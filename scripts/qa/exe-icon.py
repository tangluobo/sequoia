"""Inspect PE icon resources or update a fresh copy of an owned Windows build.

Uses Microsoft's UpdateResource API; never patches the input executable.
https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-updateresourcew
https://learn.microsoft.com/windows/win32/debug/pe-format#the-rsrc-section
"""
import argparse
import ctypes
import hashlib
import json
from pathlib import Path
import shutil
import struct


def sha(data):
    return hashlib.sha256(data).hexdigest().upper()


class PE:
    def __init__(self, path):
        self.data = Path(path).read_bytes()
        pe = struct.unpack_from("<I", self.data, 0x3C)[0]
        assert self.data[:2] == b"MZ" and self.data[pe:pe + 4] == b"PE\0\0"
        count = struct.unpack_from("<H", self.data, pe + 6)[0]
        optional_size = struct.unpack_from("<H", self.data, pe + 20)[0]
        optional = pe + 24
        magic = struct.unpack_from("<H", self.data, optional)[0]
        directories = optional + {0x20B: 112, 0x10B: 96}[magic]
        self.resource_rva = struct.unpack_from("<I", self.data, directories + 16)[0]
        self.certificate_size = struct.unpack_from("<I", self.data, directories + 36)[0]
        self.sections = {}
        for index in range(count):
            header = optional + optional_size + 40 * index
            name = self.data[header:header + 8].rstrip(b"\0").decode("ascii")
            size, rva, raw_size, raw = struct.unpack_from("<IIII", self.data, header + 8)
            self.sections[name] = (rva, size, self.data[raw:raw + raw_size], raw)
        self.resources = {}
        if self.resource_rva:
            self.root = self.file_offset(self.resource_rva)
            self.walk(0, ())

    def file_offset(self, rva):
        for address, size, data, raw in self.sections.values():
            if address <= rva < address + max(size, len(data)):
                return raw + rva - address
        raise ValueError(f"Unmapped RVA: {rva:x}")

    def walk(self, relative, path):
        assert len(path) <= 3
        directory = self.root + relative
        named, numbered = struct.unpack_from("<HH", self.data, directory + 12)
        for index in range(named + numbered):
            name, offset = struct.unpack_from("<II", self.data, directory + 16 + index * 8)
            if name & 0x80000000:
                address = self.root + (name & 0x7FFFFFFF)
                size = struct.unpack_from("<H", self.data, address)[0]
                name = self.data[address + 2:address + 2 + 2 * size].decode("utf-16-le")
            key = path + (name,)
            if offset & 0x80000000:
                self.walk(offset & 0x7FFFFFFF, key)
            else:
                rva, size = struct.unpack_from("<II", self.data, self.root + offset)
                address = self.file_offset(rva)
                self.resources[key] = self.data[address:address + size]


def icon_frames(path):
    data = Path(path).read_bytes()
    reserved, kind, count = struct.unpack_from("<HHH", data)
    assert reserved == 0 and kind == 1 and 0 < count <= 256
    frames = []
    for index in range(count):
        start = 6 + index * 16
        size, offset = struct.unpack_from("<II", data, start + 8)
        assert offset >= 6 + count * 16 and offset + size <= len(data)
        frames.append((data[start:start + 12], data[offset:offset + size]))
    return frames


def embed(source, output, icon):
    root = Path(__file__).resolve().parents[2] / "target"
    assert output.resolve().is_relative_to(root) and source.resolve().is_relative_to(root)
    assert source.resolve() != output.resolve() and not output.exists()
    before = PE(source)
    assert not before.certificate_size, "Refuse to invalidate a signed executable"
    assert not any(key[0] in (3, 14) for key in before.resources), "Expected a build without icons"
    frames = icon_frames(icon)
    output.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, output)
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.BeginUpdateResourceW.argtypes = [ctypes.c_wchar_p, ctypes.c_int]
    kernel.BeginUpdateResourceW.restype = ctypes.c_void_p
    kernel.UpdateResourceW.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ushort, ctypes.c_void_p, ctypes.c_uint32]
    kernel.UpdateResourceW.restype = ctypes.c_int
    kernel.EndUpdateResourceW.argtypes = [ctypes.c_void_p, ctypes.c_int]
    kernel.EndUpdateResourceW.restype = ctypes.c_int
    handle = kernel.BeginUpdateResourceW(str(output.resolve()), False)
    if not handle:
        raise ctypes.WinError(ctypes.get_last_error())
    try:
        group = struct.pack("<HHH", 0, 1, len(frames))
        for index, (metadata, payload) in enumerate(frames, 1):
            buffer = ctypes.create_string_buffer(payload)
            if not kernel.UpdateResourceW(handle, 3, index, 1033, buffer, len(payload)):
                raise ctypes.WinError(ctypes.get_last_error())
            group += metadata + struct.pack("<H", index)
        buffer = ctypes.create_string_buffer(group)
        if not kernel.UpdateResourceW(handle, 14, 1, 1033, buffer, len(group)):
            raise ctypes.WinError(ctypes.get_last_error())
        if not kernel.EndUpdateResourceW(handle, False):
            raise ctypes.WinError(ctypes.get_last_error())
        handle = None
    finally:
        if handle:
            kernel.EndUpdateResourceW(handle, True)
    after = PE(output)
    for name, (_, _, data, _) in before.sections.items():
        if name != ".rsrc":
            assert after.sections[name][2] == data, f"Changed executable section: {name}"
    for key, data in before.resources.items():
        assert after.resources[key] == data, f"Changed original resource: {key}"
    assert sha(source.read_bytes()) == sha(before.data), "Input executable changed"
    return {"sourceSha256": sha(before.data), "codeSectionSha256": sha(before.sections[".text"][2]), "allOriginalNonResourceSectionsUnchanged": True, "existingResourcesPreserved": True}


def verify(executable, icon):
    pe = PE(executable)
    frames = icon_frames(icon)
    groups = [(key, data) for key, data in pe.resources.items() if key[0] == 14]
    assert len(groups) == 1
    key, group = groups[0]
    assert struct.unpack_from("<HHH", group) == (0, 1, len(frames))
    sizes = []
    for index, (metadata, payload) in enumerate(frames):
        start = 6 + index * 14
        assert group[start:start + 12] == metadata
        resource_id = struct.unpack_from("<H", group, start + 12)[0]
        assert pe.resources[(3, resource_id, key[2])] == payload, "Icon frame bytes differ"
        sizes.append([metadata[0] or 256, metadata[1] or 256])
    return {"executableSha256": sha(pe.data), "iconSha256": sha(Path(icon).read_bytes()), "iconSizes": sizes, "allEmbeddedFramesMatchOriginalBytes": True}


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("executable", type=Path)
    parser.add_argument("icon", type=Path)
    parser.add_argument("--output", type=Path, help="Fresh owned copy to update; omit for inspection only")
    parser.add_argument("--proof", type=Path)
    args = parser.parse_args()
    result = embed(args.executable, args.output, args.icon) if args.output else {}
    result.update(verify(args.output or args.executable, args.icon))
    if args.proof:
        args.proof.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))
