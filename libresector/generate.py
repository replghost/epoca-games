#!/usr/bin/env python3
# SPDX-License-Identifier: CC0-1.0
"""Generate the original CC0 Epoca LibreSector compatibility data pack."""

from __future__ import annotations

import hashlib
import os
import struct
import sys
import tempfile
from collections.abc import Iterable
from decimal import ROUND_HALF_EVEN, Decimal, getcontext
from pathlib import Path

VERSION = (0, 1, 0)
EXPECTED_SHA256 = "c268b772c031170ac0889e2775b57874b0fdcd139fbde8708737c6012a753dd7"
getcontext().prec = 70
PI = Decimal(
    "3.141592653589793238462643383279502884197169399375105820974944592307816406286"
)


def decimal_sin(value: Decimal) -> Decimal:
    value %= 2 * PI
    if value > PI:
        value -= 2 * PI
    if value > PI / 2:
        value = PI - value
    elif value < -PI / 2:
        value = -PI - value

    term = value
    total = value
    order = 1
    while True:
        term *= -(value * value) / Decimal((2 * order) * (2 * order + 1))
        updated = total + term
        if updated == total:
            return total
        total = updated
        order += 1


def decimal_atan(value: Decimal) -> Decimal:
    if value < 0:
        return -decimal_atan(-value)
    if value > 1:
        return PI / 2 - decimal_atan(1 / value)
    if value > Decimal("0.5"):
        return PI / 4 + decimal_atan((value - 1) / (value + 1))

    total = Decimal(0)
    power = value
    order = 0
    while True:
        term = power / Decimal(2 * order + 1)
        updated = total - term if order & 1 else total + term
        if updated == total:
            return total
        total = updated
        power *= value * value
        order += 1


def rounded(value: Decimal) -> int:
    return int(value.to_integral_value(rounding=ROUND_HALF_EVEN))




def canvas(width: int, height: int, color: int = 0) -> list[list[int]]:
    return [[color for _ in range(width)] for _ in range(height)]


def rect(
    image: list[list[int]],
    x0: int,
    y0: int,
    x1: int,
    y1: int,
    color: int,
) -> None:
    for y in range(max(0, y0), min(len(image), y1)):
        for x in range(max(0, x0), min(len(image[0]), x1)):
            image[y][x] = color


def line(
    image: list[list[int]],
    x0: int,
    y0: int,
    x1: int,
    y1: int,
    color: int,
) -> None:
    dx = abs(x1 - x0)
    sx = 1 if x0 < x1 else -1
    dy = -abs(y1 - y0)
    sy = 1 if y0 < y1 else -1
    error = dx + dy
    while True:
        if 0 <= y0 < len(image) and 0 <= x0 < len(image[0]):
            image[y0][x0] = color
        if x0 == x1 and y0 == y1:
            return
        doubled = 2 * error
        if doubled >= dy:
            error += dy
            x0 += sx
        if doubled <= dx:
            error += dx
            y0 += sy


def seven_segment_digit(
    number: int,
    width: int = 8,
    height: int = 12,
    foreground: int = 231,
    background: int = 255,
) -> list[list[int]]:
    image = canvas(width, height, background)
    segments = {
        0: "abcedf",
        1: "bc",
        2: "abdeg",
        3: "abcdg",
        4: "bcfg",
        5: "acdfg",
        6: "acdefg",
        7: "abc",
        8: "abcdefg",
        9: "abcdfg",
    }[number]
    coordinates = {
        "a": (2, 1, width - 2, 2),
        "b": (width - 2, 2, width - 1, height // 2),
        "c": (width - 2, height // 2, width - 1, height - 2),
        "d": (2, height - 2, width - 2, height - 1),
        "e": (1, height // 2, 2, height - 2),
        "f": (1, 2, 2, height // 2),
        "g": (2, height // 2, width - 2, height // 2 + 1),
    }
    for segment in segments:
        rect(image, *coordinates[segment], foreground)
    return image


def make_palette() -> bytes:
    colors: list[tuple[int, int, int]] = []
    for index in range(16):
        value = round(index * 63 / 15)
        colors.append((value, value, value))
    for red in range(6):
        for green in range(6):
            for blue in range(6):
                colors.append(
                    (
                        round(red * 63 / 5),
                        round(green * 63 / 5),
                        round(blue * 63 / 5),
                    )
                )
    while len(colors) < 255:
        index = len(colors) - 232
        colors.append((63, min(63, index * 3), 0))
    colors.append((0, 0, 0))

    output = bytearray(component for rgb in colors for component in rgb)
    shades = 32
    output += struct.pack("<H", shades)
    for shade in range(shades):
        factor = (shades - 1 - shade) / (shades - 1)
        for index, (red, green, blue) in enumerate(colors):
            if index == 255:
                output.append(255)
                continue
            output.append(
                min(
                    range(255),
                    key=lambda candidate: (
                        (colors[candidate][0] - red * factor) ** 2
                        + (colors[candidate][1] - green * factor) ** 2
                        + (colors[candidate][2] - blue * factor) ** 2
                    ),
                )
            )
    for first in range(256):
        for second in range(256):
            if first == 255:
                output.append(second)
                continue
            if second == 255:
                output.append(first)
                continue
            blended = tuple(
                (colors[first][channel] + colors[second][channel]) // 2
                for channel in range(3)
            )
            output.append(
                min(
                    range(255),
                    key=lambda candidate: sum(
                        (colors[candidate][channel] - blended[channel]) ** 2
                        for channel in range(3)
                    ),
                )
            )
    return bytes(output)


def make_tables() -> bytes:
    output = bytearray()
    for index in range(2048):
        angle = Decimal(index) * PI / 1024
        output += struct.pack("<h", rounded(decimal_sin(angle) * 16384))
    for index in range(640):
        ratio = (Decimal(index) - Decimal("639.5")) / 160
        output += struct.pack(
            "<h",
            rounded(decimal_atan(ratio) * 1024 / PI * 64),
        )
    output += bytes(1024)  # text font
    output += bytes(1024)  # small text font
    output += bytes(range(256)) * 4
    return bytes(output)



def make_art() -> bytes:
    tiles: dict[int, list[list[int]]] = {}

    image = canvas(64, 64, 52)
    for y in range(0, 64, 16):
        for x in range(-16 if (y // 16) % 2 else 0, 64, 32):
            rect(image, x + 1, y + 1, x + 30, y + 14, 88 if (x + y) // 16 % 2 else 124)
    tiles[0] = image

    image = canvas(64, 64, 17)
    for y in range(64):
        for x in range(64):
            image[y][x] = 22 if (x // 8 + y // 8) & 1 else 23
    tiles[1] = image

    image = canvas(64, 64, 18)
    for y in range(8, 64, 16):
        for x in range(8, 64, 16):
            image[y][x] = 231
    tiles[2] = image

    image = canvas(64, 64, 196)
    for radius, color in ((28, 231), (22, 196), (16, 231), (10, 196), (4, 231)):
        for angle in range(360):
            radians = Decimal(angle) * PI / 180
            x = 32 + rounded(decimal_sin(PI / 2 + radians) * radius)
            y = 32 + rounded(decimal_sin(radians) * radius)
            if 0 <= x < 64 and 0 <= y < 64:
                image[y][x] = color
    rect(image, 30, 4, 34, 60, 231)
    rect(image, 4, 30, 60, 34, 231)
    tiles[3] = image

    for tile_id, color in ((30, 46), (31, 196), (33, 21)):
        image = canvas(24, 24, 255)
        rect(image, 1, 1, 23, 23, color)
        rect(image, 4, 4, 20, 20, 0)
        tiles[tile_id] = image

    image = canvas(32, 48, 255)
    rect(image, 12, 5, 20, 17, 214)
    rect(image, 9, 17, 23, 35, 21)
    rect(image, 5, 20, 9, 38, 214)
    rect(image, 23, 20, 27, 38, 214)
    rect(image, 10, 35, 15, 48, 52)
    rect(image, 18, 35, 23, 48, 52)
    tiles[1405] = image

    image = canvas(320, 34, 17)
    rect(image, 0, 0, 320, 2, 46)
    rect(image, 5, 5, 90, 30, 20)
    rect(image, 96, 5, 224, 30, 20)
    rect(image, 230, 5, 315, 30, 20)
    tiles[2462] = image

    for number in range(10):
        tiles[2472 + number] = seven_segment_digit(number)

    image = canvas(9, 9, 255)
    line(image, 4, 0, 4, 8, 231)
    line(image, 0, 4, 8, 4, 231)
    image[4][4] = 255
    tiles[2523] = image

    for frame in range(12):
        image = canvas(64, 72, 255)
        recoil = max(0, 4 - abs(frame - 3))
        y = 24 + recoil
        rect(image, 24, y, 42, y + 34, 52)
        rect(image, 17, y + 5, 42, y + 18, 44)
        rect(image, 18, y + 7, 38, y + 12, 231)
        rect(image, 28, y + 34, 38, 72, 88)
        tiles[2524 + frame] = image

    for number in range(10):
        tiles[3010 + number] = seven_segment_digit(number, 5, 7, 231, 255)

    image = canvas(320, 200, 17)
    rect(image, 12, 12, 308, 188, 20)
    rect(image, 18, 18, 302, 182, 17)
    rect(image, 28, 70, 292, 130, 21)
    tiles[3281] = image

    tile_end = 9215
    widths = [0] * (tile_end + 1)
    heights = [0] * (tile_end + 1)
    animation = [0] * (tile_end + 1)
    pixels = bytearray()
    for tile_id, tile in sorted(tiles.items()):
        heights[tile_id] = len(tile)
        widths[tile_id] = len(tile[0])

    output = bytearray(struct.pack("<iiii", 1, tile_end + 1, 0, tile_end))
    output += struct.pack(f"<{tile_end + 1}H", *widths)
    output += struct.pack(f"<{tile_end + 1}H", *heights)
    output += struct.pack(f"<{tile_end + 1}I", *animation)
    for tile_id in range(tile_end + 1):
        if tile_id not in tiles:
            continue
        tile = tiles[tile_id]
        height = len(tile)
        width = len(tile[0])
        pixels.extend(tile[y][x] for x in range(width) for y in range(height))
    output += pixels
    return bytes(output)


def make_map() -> bytes:
    output = bytearray(struct.pack("<i", 7))
    output += struct.pack("<iiihh", 0, 0, -8192, 0, 0)
    output += struct.pack("<h", 1)
    sector = (
        0, 4, -16384, 0, 0, 2, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0,
        0, 0, -1,
    )
    output += struct.pack("<hhiihhhhbBBBhhbBBBBBhhh", *sector)
    points = ((-4096, -4096), (4096, -4096), (4096, 4096), (-4096, 4096))
    output += struct.pack("<h", len(points))
    for index, (x, y) in enumerate(points):
        picture = 3 if index == 1 else 0
        wall = (
            x, y, (index + 1) % 4, -1, -1, 0, picture, -1, 0, 0, 8, 8, 0, 0, 0,
            0, -1,
        )
        output += struct.pack("<iihhhhhhbBBBBBhhh", *wall)
    output += struct.pack("<h", 0)
    return bytes(output)


def make_lookup() -> bytes:
    identity = bytes(range(256))
    return (
        bytes([31])
        + b"".join(bytes([index]) + identity for index in range(1, 32))
        + bytes(768 * 5)
    )


def make_group(files: Iterable[tuple[str, bytes]]) -> bytes:
    entries = list(files)
    output = bytearray(b"KenSilverman" + struct.pack("<I", len(entries)))
    for name, data in entries:
        encoded = name.encode("ascii")
        if not encoded or len(encoded) > 12:
            raise ValueError(f"invalid GRP entry name: {name}")
        output += encoded.ljust(12, b"\0") + struct.pack("<I", len(data))
    for _, data in entries:
        output += data
    return bytes(output)


def generate() -> bytes:
    game_con = (
        b"// Original minimal configuration for the Epoca libre proof pack.\n"
        b"gamestartup 32 10 100 100 768 768 53248 176 2048 2048 2048 2048 "
        b"2048 2048 2048 200 50 200 50 50 50 50 10 50 50 1 3 0 64 0\n"
    )
    return make_group(
        (
            ("LIBRE.PACK", b"Epoca LibreSector Pack\0CC0-1.0\0"),
            ("GAME.CON", game_con),
            ("TABLES.DAT", make_tables()),
            ("PALETTE.DAT", make_palette()),
            ("LOOKUP.DAT", make_lookup()),
            ("TILES000.ART", make_art()),
            ("E1L1.MAP", make_map()),
        )
    )


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {Path(sys.argv[0]).name} OUTPUT")
    output = Path(sys.argv[1])
    data = generate()
    digest = hashlib.sha256(data).hexdigest()
    if digest != EXPECTED_SHA256:
        raise SystemExit(
            f"generated GRP SHA-256 mismatch: expected {EXPECTED_SHA256}, got {digest}"
        )
    output.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary = tempfile.mkstemp(prefix=f".{output.name}.", dir=output.parent)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(data)
        os.chmod(temporary, 0o444)
        os.replace(temporary, output)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)
    print(f"Created {output} ({len(data)} bytes, sha256:{digest})")


if __name__ == "__main__":
    main()
