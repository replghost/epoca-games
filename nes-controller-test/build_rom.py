#!/usr/bin/env python3
"""Build the MIT-licensed Epoca NES controller/audio validation cartridge."""

from __future__ import annotations

import struct
import sys
from pathlib import Path

ORIGIN = 0xC000
PRG_SIZE = 16 * 1024


class Assembler:
    def __init__(self) -> None:
        self.code = bytearray()
        self.labels: dict[str, int] = {}
        self.absolute_fixups: list[tuple[int, str]] = []
        self.relative_fixups: list[tuple[int, str]] = []

    def label(self, name: str) -> None:
        if name in self.labels:
            raise ValueError(f"duplicate label: {name}")
        self.labels[name] = len(self.code)

    def emit(self, *values: int) -> None:
        self.code.extend(value & 0xFF for value in values)

    def absolute(self, opcode: int, target: int | str) -> None:
        self.emit(opcode)
        if isinstance(target, str):
            self.absolute_fixups.append((len(self.code), target))
            self.emit(0, 0)
        else:
            self.emit(target, target >> 8)

    def absolute_x(self, opcode: int, target: str) -> None:
        self.absolute(opcode, target)

    def branch(self, opcode: int, target: str) -> None:
        self.emit(opcode)
        self.relative_fixups.append((len(self.code), target))
        self.emit(0)

    def finish(self) -> bytearray:
        for offset, label in self.absolute_fixups:
            address = ORIGIN + self.labels[label]
            self.code[offset : offset + 2] = struct.pack("<H", address)
        for offset, label in self.relative_fixups:
            displacement = self.labels[label] - (offset + 1)
            if not -128 <= displacement <= 127:
                raise ValueError(f"branch to {label} is out of range: {displacement}")
            self.code[offset] = displacement & 0xFF
        return self.code


def build_program() -> tuple[bytes, dict[str, int]]:
    a = Assembler()
    a.label("reset")
    a.emit(0x78, 0xD8)  # SEI; CLD
    a.emit(0xA2, 0xFF, 0x9A, 0xE8)  # LDX #$ff; TXS; INX
    a.absolute(0x8E, 0x2000)  # STX PPUCTRL
    a.absolute(0x8E, 0x2001)  # STX PPUMASK
    a.absolute(0x8E, 0x4010)  # STX DMC control

    a.label("vblank_1")
    a.absolute(0x2C, 0x2002)
    a.branch(0x10, "vblank_1")  # BPL
    a.label("vblank_2")
    a.absolute(0x2C, 0x2002)
    a.branch(0x10, "vblank_2")

    # Upload tile 1 into CHR RAM at $0010.
    a.emit(0xA9, 0x00)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA9, 0x10)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA2, 0x00)
    a.label("chr_loop")
    a.absolute_x(0xBD, "tile")
    a.absolute(0x8D, 0x2007)
    a.emit(0xE8, 0xE0, 0x10)  # INX; CPX #16
    a.branch(0xD0, "chr_loop")

    # Upload 32-byte palette.
    a.emit(0xA9, 0x3F)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA9, 0x00)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA2, 0x00)
    a.label("palette_loop")
    a.absolute_x(0xBD, "palette")
    a.absolute(0x8D, 0x2007)
    a.emit(0xE8, 0xE0, 0x20)
    a.branch(0xD0, "palette_loop")

    # Fill nametable and attributes with tile 1.
    a.emit(0xA9, 0x20)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA9, 0x00)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA9, 0x01, 0xA2, 0x04, 0xA0, 0x00)  # LDA #1; LDX #4; LDY #0
    a.label("fill_inner")
    a.absolute(0x8D, 0x2007)
    a.emit(0x88)  # DEY
    a.branch(0xD0, "fill_inner")
    a.emit(0xCA)  # DEX
    a.branch(0xD0, "fill_inner")

    # Continuous pulse tone. Bit 5 halts the length counter so the channel
    # remains audible after the initial validation frames.
    for address, value in ((0x4015, 0x01), (0x4000, 0xBF), (0x4001, 0x00), (0x4002, 0xFD), (0x4003, 0x08)):
        a.emit(0xA9, value)
        a.absolute(0x8D, address)

    a.emit(0xA9, 0x80)
    a.absolute(0x8D, 0x2000)  # enable NMI
    a.emit(0xA9, 0x08)
    a.absolute(0x8D, 0x2001)  # show background
    a.label("main")
    a.absolute(0x4C, "main")

    a.label("nmi")
    a.emit(0x48, 0x8A, 0x48, 0x98, 0x48)  # save A/X/Y
    a.absolute(0xEE, 0x6000)  # battery-backed frame counter
    a.emit(0xA9, 0x01)
    a.absolute(0x8D, 0x4016)
    a.emit(0xA9, 0x00)
    a.absolute(0x8D, 0x4016)

    # Read controller 1 into $00.
    a.emit(0xA2, 0x08, 0xA9, 0x00, 0x85, 0x00)
    a.label("read_p1")
    a.absolute(0xAD, 0x4016)
    a.emit(0x4A, 0x26, 0x00, 0xCA)
    a.branch(0xD0, "read_p1")

    # Read controller 2 into $01.
    a.emit(0xA2, 0x08, 0xA9, 0x00, 0x85, 0x01)
    a.label("read_p2")
    a.absolute(0xAD, 0x4017)
    a.emit(0x4A, 0x26, 0x01, 0xCA)
    a.branch(0xD0, "read_p2")

    a.emit(0xA5, 0x00, 0x05, 0x01)  # LDA $00; ORA $01
    a.branch(0xF0, "idle_color")
    a.emit(0xA9, 0x16)  # active red
    a.absolute(0x4C, "set_color")
    a.label("idle_color")
    a.emit(0xA9, 0x21)  # idle blue
    a.label("set_color")
    a.emit(0x85, 0x02)
    a.emit(0xA9, 0x3F)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA9, 0x00)
    a.absolute(0x8D, 0x2006)
    a.emit(0xA5, 0x02)
    a.absolute(0x8D, 0x2007)

    # Pressing either controller changes pitch as well as colour.
    a.emit(0xA5, 0x00, 0x05, 0x01)
    a.branch(0xF0, "idle_pitch")
    a.emit(0xA9, 0x40)
    a.absolute(0x4C, "set_pitch")
    a.label("idle_pitch")
    a.emit(0xA9, 0xFD)
    a.label("set_pitch")
    a.absolute(0x8D, 0x4002)

    a.emit(0xA9, 0x00)
    a.absolute(0x8D, 0x2005)
    a.absolute(0x8D, 0x2005)
    a.emit(0x68, 0xA8, 0x68, 0xAA, 0x68, 0x40)  # restore Y/X/A; RTI

    a.label("irq")
    a.emit(0x40)
    a.label("tile")
    a.emit(0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55, 0xAA, 0x55)
    a.emit(0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00)
    a.label("palette")
    a.emit(
        0x0F, 0x21, 0x16, 0x30, 0x0F, 0x06, 0x16, 0x26,
        0x0F, 0x09, 0x19, 0x29, 0x0F, 0x01, 0x11, 0x21,
        0x0F, 0x21, 0x16, 0x30, 0x0F, 0x06, 0x16, 0x26,
        0x0F, 0x09, 0x19, 0x29, 0x0F, 0x01, 0x11, 0x21,
    )
    return bytes(a.finish()), a.labels


def build_rom() -> bytes:
    program, labels = build_program()
    if len(program) > PRG_SIZE - 6:
        raise ValueError("program does not fit in NROM-128 PRG")
    prg = bytearray([0xEA] * PRG_SIZE)
    prg[: len(program)] = program
    for offset, label in ((0x3FFA, "nmi"), (0x3FFC, "reset"), (0x3FFE, "irq")):
        prg[offset : offset + 2] = struct.pack("<H", ORIGIN + labels[label])
    # iNES 1.0: 16 KiB PRG, CHR RAM, horizontal mirroring, battery RAM.
    header = b"NES\x1a" + bytes((1, 0, 0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0))
    return header + bytes(prg)


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit(f"usage: {sys.argv[0]} OUTPUT")
    output = Path(sys.argv[1])
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = output.with_name(f".{output.name}.tmp")
    temporary.write_bytes(build_rom())
    temporary.replace(output)


if __name__ == "__main__":
    main()
