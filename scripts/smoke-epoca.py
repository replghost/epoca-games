#!/usr/bin/env python3
import os
import select
import struct
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
HOST = os.environ.get("EPOCA_PVM_HOST")
if not HOST:
    raise SystemExit("EPOCA_PVM_HOST must point to the epoca-pvm-host binary")

APPS = {
    "doom": "framebuffer",
    "quake": "framebuffer",
    "duke3d": "framebuffer",
    "nes": "framebuffer",
    "egui-kitchen-sink": "tri2d",
    "gpu-cube": "webgpu-raster",
    "scene-lab": "webgpu-raster",
}
def main():
    selected = sys.argv[1:] or list(APPS)
    for app in selected:
        profile = APPS.get(app)
        if profile is None:
            raise SystemExit(f"unknown app: {app}")
        bundle = ROOT / "apps" / app / "bundle"
        command = [HOST, "serve", profile, str(bundle / "app.polkavm"), str(bundle)]
        child = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE)
        try:
            ready = next_message(child, 1)
            if len(ready) < 5:
                raise RuntimeError(f"{app}: malformed ready message")
            if profile == "webgpu-raster":
                write_message(child, 2, gpu_capabilities())
                batch = next_message(child, 7)
                if batch[1:5] != b"EPG1":
                    raise RuntimeError(f"{app}: malformed GPU batch")
            elif profile == "tri2d":
                mesh = next_message(child, 6)
                if mesh[1:5] != b"ETD1":
                    raise RuntimeError(f"{app}: malformed Tri2D stream")
            else:
                frame = next_message(child, 2)
                if len(frame) <= 13:
                    raise RuntimeError(f"{app}: empty framebuffer")
            print(f"{app}: {profile} smoke passed")
        finally:
            if child.stdin:
                child.stdin.close()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
            if child.returncode not in (0, -9):
                raise RuntimeError(f"{app}: host exited with {child.returncode}")


def read_exact(stream, length, timeout=10):
    output = bytearray()
    while len(output) < length:
        ready, _, _ = select.select([stream], [], [], timeout)
        if not ready:
            raise TimeoutError("host message timed out")
        chunk = os.read(stream.fileno(), length - len(output))
        if not chunk:
            raise EOFError("host closed its output")
        output.extend(chunk)
    return bytes(output)


def next_message(child, message_type):
    while True:
        length = struct.unpack("<I", read_exact(child.stdout, 4))[0]
        if length < 1 or length > 8 * 1024 * 1024 + 1:
            raise RuntimeError(f"invalid host message length: {length}")
        message = read_exact(child.stdout, length)
        if message[0] == message_type:
            return message


def write_message(child, message_type, payload):
    message = bytes([message_type]) + payload
    child.stdin.write(struct.pack("<I", len(message)) + message)
    child.stdin.flush()


def gpu_capabilities():
    limits = [
        4096,
        16 * 1024 * 1024,
        16,
        4,
        8,
        16,
        4,
        256 * 1024 * 1024,
        64 * 1024 * 1024,
        8192,
        4 * 1024 * 1024,
        16 * 1024 * 1024,
    ]
    output = bytearray(56 + len(limits) * 16)
    output[0:4] = b"EGC1"
    struct.pack_into("<H", output, 4, 1)
    struct.pack_into("<I", output, 8, len(output))
    struct.pack_into("<H", output, 12, 3)
    for offset, value in ((16, 640), (20, 480), (24, 640), (28, 480), (36, 1), (40, 1)):
        struct.pack_into("<I", output, offset, value)
    struct.pack_into("<f", output, 32, 1.0)
    struct.pack_into("<I", output, 44, len(limits))
    for index, value in enumerate(limits):
        offset = 56 + index * 16
        struct.pack_into("<H", output, offset, index + 1)
        struct.pack_into("<Q", output, offset + 4, value)
    return bytes(output)

if __name__ == "__main__":
    main()
