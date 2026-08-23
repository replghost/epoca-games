#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

"""Run the packaged NES engine through Epoca's browser-owned PolkaVM JIT."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
from pathlib import Path

PRODUCT_ID = "nes-browser-regression"
PRODUCT_URL = f"dot://{PRODUCT_ID}.paseo/"


class Marionette:
    def __init__(self, port: int) -> None:
        self.port = port
        self.socket: socket.socket | None = None
        self.message_id = 0

    def connect(self, timeout: float = 180) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                self.socket = socket.create_connection(("127.0.0.1", self.port), timeout=5)
                break
            except OSError:
                time.sleep(0.2)
        if self.socket is None:
            raise RuntimeError(f"Marionette port {self.port} never accepted")
        self.socket.settimeout(30)
        self._receive()
        self.command("WebDriver:NewSession", {})
        self.command("WebDriver:SetTimeouts", {"pageLoad": 120_000, "script": 120_000})
        self.set_context("content")

    def _receive(self):
        assert self.socket is not None
        prefix = bytearray()
        while b":" not in prefix:
            chunk = self.socket.recv(1)
            if not chunk:
                raise RuntimeError("Marionette closed the connection")
            prefix.extend(chunk)
        raw_length, initial = bytes(prefix).split(b":", 1)
        length = int(raw_length)
        payload = bytearray(initial)
        while len(payload) < length:
            chunk = self.socket.recv(length - len(payload))
            if not chunk:
                raise RuntimeError("Marionette closed mid-message")
            payload.extend(chunk)
        return json.loads(payload.decode())

    def command(self, name: str, parameters: dict, timeout: float = 120):
        assert self.socket is not None
        self.message_id += 1
        payload = json.dumps([0, self.message_id, name, parameters]).encode()
        self.socket.sendall(f"{len(payload)}:".encode() + payload)
        self.socket.settimeout(timeout)
        while True:
            message = self._receive()
            if (
                isinstance(message, list)
                and len(message) == 4
                and message[0] == 1
                and message[1] == self.message_id
            ):
                error, result = message[2], message[3]
                if error:
                    raise RuntimeError(f"{name}: {error}")
                return result

    def set_context(self, context: str) -> None:
        self.command("Marionette:SetContext", {"value": context})

    def script(self, body: str):
        result = self.command("WebDriver:ExecuteScript", {"script": body, "args": []})
        return result.get("value") if isinstance(result, dict) else result

    def close(self) -> None:
        if self.socket is None:
            return
        try:
            self.command("WebDriver:DeleteSession", {})
        except Exception:
            pass
        self.socket.close()
        self.socket = None


def free_port() -> int:
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def write_preferences(profile: Path, port: int) -> None:
    preferences = {
        "marionette.port": port,
        "browser.newtabpage.enabled": False,
        "browser.startup.homepage": "about:blank",
        "browser.startup.page": 0,
        "browser.shell.checkDefaultBrowser": False,
        "browser.backup.enabled": False,
        "security.sandbox.content.level": 0,
        "epoca.pvm.backend": "browser",
        "zen.welcome-screen.seen": True,
        "epoca.welcome.seen": True,
    }
    with (profile / "user.js").open("w", encoding="utf-8") as output:
        for key, value in preferences.items():
            output.write(f"user_pref({json.dumps(key)}, {json.dumps(value)});\n")


def register_product(client: Marionette, engine: Path, cartridge: Path) -> None:
    client.set_context("chrome")
    result = client.command(
        "WebDriver:ExecuteAsyncScript",
        {
            "script": """
const [engineRoot, cartridgePath, productId] = arguments;
const done = arguments[arguments.length - 1];
(async () => {
  const { EpocaDotAppRegistry } = ChromeUtils.importESModule(
    "resource:///modules/EpocaDotAppRegistry.sys.mjs"
  );
  const assets = Object.create(null);
  for (const path of ["/manifest.json", "/app.polkavm", "/index.html"]) {
    let source = PathUtils.join(engineRoot, ...path.slice(1).split("/"));
    if (path === "/index.html" && !(await IOUtils.exists(source))) {
      source = PathUtils.join(PathUtils.parent(engineRoot), "index.html");
    }
    assets[path] = await IOUtils.read(source);
  }
  assets["/game/cartridge.nes"] = await IOUtils.read(cartridgePath);
  EpocaDotAppRegistry.register(productId, assets);
  done(true);
})().catch(error => done({ error: error.message }));
""",
            "args": [str(engine), str(cartridge), PRODUCT_ID],
        },
    )
    if result.get("value") is not True:
        raise RuntimeError(f"could not register local NES package: {result}")
    client.set_context("content")


def snapshot(client: Marionette) -> dict:
    return client.script(
        """
const canvas = document.getElementById("pvm-canvas");
let hash = 2166136261;
let nonblank = 0;
if (canvas?.width && canvas?.height) {
  const pixels = canvas.getContext("2d").getImageData(0, 0, canvas.width, canvas.height).data;
  for (let index = 0; index < pixels.length; index += 16) {
    const value = pixels[index] | pixels[index + 1] | pixels[index + 2];
    if (value) nonblank++;
    hash = Math.imul(hash ^ pixels[index], 16777619) >>> 0;
    hash = Math.imul(hash ^ pixels[index + 1], 16777619) >>> 0;
    hash = Math.imul(hash ^ pixels[index + 2], 16777619) >>> 0;
  }
}
return {
  ready: canvas?.dataset.pvmReady || "",
  frames: Number(canvas?.dataset.pvmFrames || 0),
  audioSamples: Number(canvas?.dataset.pvmAudioSamples || 0),
  audioNonzero: canvas?.dataset.pvmAudioNonzero || "",
  runtime: document.getElementById("pvm-runtime")?.textContent || "",
  status: document.getElementById("pvm-status")?.textContent || "",
  nonblank,
  hash,
};
"""
    )


def wait_for_runtime(client: Marionette, minimum_frames: int, timeout: float = 120) -> dict:
    deadline = time.monotonic() + timeout
    state = None
    while time.monotonic() < deadline:
        state = snapshot(client)
        if state["status"] and state["status"] != "Starting PolkaVM application…":
            raise RuntimeError(f"NES runtime failed: {state}")
        if state["ready"] == "true" and state["frames"] >= minimum_frames and state["nonblank"]:
            return state
        time.sleep(0.2)
    raise RuntimeError(f"NES runtime did not become ready: {state}")


def key(client: Marionette, value: str, down: bool) -> None:
    client.command(
        "WebDriver:PerformActions",
        {
            "actions": [
                {
                    "type": "key",
                    "id": "keyboard",
                    "actions": [{"type": "keyDown" if down else "keyUp", "value": value}],
                }
            ]
        },
    )

def activate_audio(client: Marionette) -> None:
    client.command(
        "WebDriver:PerformActions",
        {
            "actions": [
                {
                    "type": "key",
                    "id": "activation",
                    "actions": [
                        {"type": "keyDown", "value": "d"},
                        {"type": "pause", "duration": 250},
                        {"type": "keyUp", "value": "d"},
                    ],
                }
            ]
        },
    )


def save_file(profile: Path) -> Path | None:
    files = list((profile / "epoca-pvm-saves").glob(f"{PRODUCT_ID}-*.sav"))
    return files[0] if len(files) == 1 else None


def stop_and_read_save(client: Marionette, profile: Path, previous: bytes | None) -> tuple[Path, bytes]:
    client.command("WebDriver:Navigate", {"url": "about:blank"})
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        path = save_file(profile)
        if path is not None and path.stat().st_size == 8192:
            data = path.read_bytes()
            if previous is None or data != previous:
                return path, data
        time.sleep(0.1)
    raise RuntimeError("NES battery save was not persisted")


def run_once(
    binary: Path,
    engine: Path,
    cartridge: Path,
    profile: Path,
    minimum_frames: int,
    exercise: bool,
) -> dict:
    previous_path = save_file(profile)
    previous = previous_path.read_bytes() if previous_path else None
    port = free_port()
    write_preferences(profile, port)
    environment = dict(os.environ)
    environment.update({"MOZ_MARIONETTE": "1", "MOZ_HEADLESS": "1"})
    with (profile / "browser.log").open("a", encoding="utf-8") as log:
        process = subprocess.Popen(
            [
                str(binary),
                "-profile",
                str(profile),
                "-marionette",
                "-remote-allow-system-access",
                "--new-instance",
            ],
            env=environment,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
    client = Marionette(port)
    try:
        client.connect()
        register_product(client, engine, cartridge)
        client.command("WebDriver:Navigate", {"url": PRODUCT_URL})
        initial = wait_for_runtime(client, minimum_frames)
        if initial["runtime"] != "POLKAVM / JIT":
            raise RuntimeError(f"unexpected runtime backend: {initial}")
        result = {"initial": initial}
        if exercise:
            started = time.monotonic()
            initial_frames = initial["frames"]
            time.sleep(2)
            clocked = snapshot(client)
            fps = (clocked["frames"] - initial_frames) / (time.monotonic() - started)
            if not 50 <= fps <= 75:
                raise RuntimeError(f"NES frame rate out of range: {fps}")
            activate_audio(client)
            key(client, "d", True)
            deadline = time.monotonic() + 10
            audio = snapshot(client)
            while time.monotonic() < deadline and (
                audio["audioSamples"] == 0 or audio["audioNonzero"] != "true"
            ):
                time.sleep(0.2)
                audio = snapshot(client)
            player_one = snapshot(client)
            key(client, "d", False)
            key(client, "\ue014", True)
            time.sleep(0.4)
            player_two = snapshot(client)
            key(client, "\ue014", False)
            if player_one["hash"] == initial["hash"] or player_two["hash"] == initial["hash"]:
                raise RuntimeError("one or both NES controllers did not alter the frame")
            if audio["audioSamples"] == 0 or audio["audioNonzero"] != "true":
                raise RuntimeError(f"NES PCM remained silent: {audio}")
            result.update(
                {
                    "fps": fps,
                    "playerOneHash": player_one["hash"],
                    "playerTwoHash": player_two["hash"],
                    "audio": audio,
                }
            )
        path, saved = stop_and_read_save(client, profile, previous)
        result.update({"save": str(path), "counter": saved[0]})
        return result
    finally:
        client.close()
        try:
            process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=20)


def process_tree_rss_kib(root_pid: int) -> int:
    pending = [root_pid]
    visited = set()
    total = 0
    while pending:
        pid = pending.pop()
        if pid in visited:
            continue
        visited.add(pid)
        status = Path(f"/proc/{pid}/status")
        try:
            for line in status.read_text(encoding="utf-8").splitlines():
                if line.startswith("VmRSS:"):
                    total += int(line.split()[1])
                    break
            children = Path(f"/proc/{pid}/task/{pid}/children")
            pending.extend(int(child) for child in children.read_text().split())
        except (FileNotFoundError, ProcessLookupError):
            continue
    return total


def run_soak(
    binary: Path,
    engine: Path,
    cartridge: Path,
    profile: Path,
    duration: int,
) -> dict:
    port = free_port()
    write_preferences(profile, port)
    environment = dict(os.environ)
    environment.update({"MOZ_MARIONETTE": "1", "MOZ_HEADLESS": "1"})
    with (profile / "browser.log").open("a", encoding="utf-8") as log:
        process = subprocess.Popen(
            [
                str(binary),
                "-profile",
                str(profile),
                "-marionette",
                "-remote-allow-system-access",
                "--new-instance",
            ],
            env=environment,
            stdout=log,
            stderr=subprocess.STDOUT,
        )
    client = Marionette(port)
    try:
        client.connect()
        register_product(client, engine, cartridge)
        client.command("WebDriver:Navigate", {"url": PRODUCT_URL})
        initial = wait_for_runtime(client, 30)
        activate_audio(client)
        deadline = time.monotonic() + 10
        started_state = snapshot(client)
        while time.monotonic() < deadline and (
            started_state["audioSamples"] == 0
            or started_state["audioNonzero"] != "true"
        ):
            time.sleep(0.2)
            started_state = snapshot(client)
        if started_state["audioNonzero"] != "true":
            raise RuntimeError(f"NES PCM remained silent before soak: {started_state}")

        started = time.monotonic()
        samples = []
        while True:
            elapsed = time.monotonic() - started
            if elapsed >= duration:
                break
            time.sleep(min(10.0, duration - elapsed))
            state = snapshot(client)
            if state["status"] or state["runtime"] != "POLKAVM / JIT":
                raise RuntimeError(f"NES runtime failed during soak: {state}")
            saved = save_file(profile)
            samples.append(
                {
                    "elapsedSeconds": time.monotonic() - started,
                    "frames": state["frames"],
                    "audioSamples": state["audioSamples"],
                    "rssKiB": process_tree_rss_kib(process.pid),
                    "saveCounter": saved.read_bytes()[0] if saved else None,
                }
            )

        final = snapshot(client)
        elapsed = time.monotonic() - started
        fps = (final["frames"] - started_state["frames"]) / elapsed
        if not 50 <= fps <= 75:
            raise RuntimeError(f"NES soak frame rate out of range: {fps}")
        if (
            final["audioNonzero"] != "true"
            or final["audioSamples"] <= started_state["audioSamples"]
        ):
            raise RuntimeError(f"NES audio stopped during soak: {final}")
        memory = [sample["rssKiB"] for sample in samples]
        memory_span = max(memory) - min(memory)
        memory_delta = memory[-1] - memory[0]
        if memory_delta > 256 * 1024:
            raise RuntimeError(
                f"NES browser process tree retained {memory_delta / 1024:.1f} MiB"
            )
        saved_path, saved = stop_and_read_save(client, profile, None)
        return {
            "durationSeconds": elapsed,
            "fps": fps,
            "initial": initial,
            "final": final,
            "memoryMinMiB": min(memory) / 1024,
            "memoryMaxMiB": max(memory) / 1024,
            "memorySpanMiB": memory_span / 1024,
            "memoryDeltaMiB": memory_delta / 1024,
            "save": str(saved_path),
            "saveCounter": saved[0],
            "samples": samples,
        }
    finally:
        client.close()
        try:
            process.wait(timeout=20)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=20)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    parser.add_argument("--engine", type=Path, default=Path(__file__).with_name("bundle"))
    parser.add_argument(
        "--cartridge",
        type=Path,
        default=Path(__file__).parents[1] / "nes-controller-test" / "bundle" / "controller-test.nes",
    )
    parser.add_argument("--keep-profile", action="store_true")
    parser.add_argument("--soak-seconds", type=int, default=0)
    arguments = parser.parse_args()
    binary = arguments.binary.resolve(strict=True)
    engine = arguments.engine.resolve(strict=True)
    cartridge = arguments.cartridge.resolve(strict=True)
    cache = Path.home() / ".cache"
    profile = Path(tempfile.mkdtemp(prefix="epoca-nes-browser-", dir=cache))
    try:
        if arguments.soak_seconds:
            if arguments.soak_seconds < 30:
                raise ValueError("--soak-seconds must be at least 30")
            print(
                json.dumps(
                    run_soak(
                        binary,
                        engine,
                        cartridge,
                        profile,
                        arguments.soak_seconds,
                    ),
                    indent=2,
                )
            )
        else:
            first = run_once(binary, engine, cartridge, profile, 60, True)
            second = run_once(binary, engine, cartridge, profile, 90, False)
            advance = (second["counter"] - first["counter"]) % 256
            if not 5 <= advance <= 180:
                raise RuntimeError(
                    f"NES save did not restore and advance: {first['counter']} -> {second['counter']}"
                )
            print(
                json.dumps(
                    {"first": first, "second": second, "saveAdvance": advance},
                    indent=2,
                )
            )
    finally:
        if arguments.keep_profile:
            print(f"Profile retained at {profile}")
        else:
            shutil.rmtree(profile, ignore_errors=True)


if __name__ == "__main__":
    main()
