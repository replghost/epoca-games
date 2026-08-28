#!/usr/bin/env python3
import binascii
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
COLORS = {
    "duke3d": (218, 154, 35),
    "nes": (211, 47, 47),
    "egui-kitchen-sink": (99, 102, 241),
    "gpu-cube": (35, 170, 210),
    "scene-lab": (49, 190, 116),
}


def chunk(kind: bytes, data: bytes) -> bytes:
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", binascii.crc32(kind + data) & 0xFFFFFFFF)
    )


def png(color: tuple[int, int, int]) -> bytes:
    width = height = 64
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            shade = 18 if (x // 8 + y // 8) % 2 else 0
            rows.extend((*[min(255, channel + shade) for channel in color], 255))
    header = b"\x89PNG\r\n\x1a\n"
    return header + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(bytes(rows), 9)) + chunk(b"IEND", b"")


for app, color in COLORS.items():
    destination = ROOT / "apps" / app / "icon.png"
    destination.write_bytes(png(color))
    print(destination.relative_to(ROOT))
