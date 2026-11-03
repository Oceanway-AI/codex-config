#!/usr/bin/env python3
"""Offline MCP smoke check of a built application executable, with no paid calls."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time


TOOLS = {"generate_images", "get_image_job", "cancel_image_job", "retry_image_job", "get_video_models", "generate_video", "get_video_job", "get_image_models"}
FIXTURE_KEY = "oceanway-packaged-smoke-not-a-real-key"


def snapshot(directory):
    return {
        str(path.relative_to(directory)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in directory.rglob("*")
        if path.is_file()
    }


def check(executable):
    executable = executable.resolve(strict=True)
    if not executable.is_file():
        raise ValueError("--executable must name the built application file")

    with tempfile.TemporaryDirectory(prefix="Oceanway MCP smoke 中文 ") as temporary:
        root = Path(temporary)
        home = root / "application data 空格"
        cwd = root / "unrelated working directory"
        home.mkdir()
        cwd.mkdir()
        (home / "image-provider.json").write_text(
            json.dumps({
                "version": 1,
                "apiKey": FIXTURE_KEY,
                "defaultModel": None,
                "allowedModels": [],
            }) + "\n",
            encoding="utf-8",
        )
        before = snapshot(home)
        environment = os.environ.copy()
        environment["OCEANWAY_CONFIG_HOME"] = str(home)
        # The process must not need Node/Python/package-manager lookup at runtime.
        environment["PATH"] = str(root / "no developer runtimes here")
        process = subprocess.Popen(
            [str(executable), "--image-mcp-stdio"],
            cwd=cwd,
            env=environment,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            errors="strict",
            creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0,
        )
        messages = queue.Queue()
        errors = []
        transcript = []

        def read_stdout():
            try:
                for line in process.stdout:
                    transcript.append(line)
                    try:
                        messages.put(json.loads(line))
                    except json.JSONDecodeError:
                        messages.put(RuntimeError("Non-JSON output contaminated MCP stdout"))
                messages.put(EOFError("MCP stdout closed before the requested response"))
            except Exception as error:
                messages.put(error)

        def read_stderr():
            for line in process.stderr:
                if sum(map(len, errors)) < 16384:
                    errors.append(line)

        readers = [threading.Thread(target=read_stdout), threading.Thread(target=read_stderr)]
        for reader in readers:
            reader.start()

        def send(message):
            process.stdin.write(json.dumps(message) + "\n")
            process.stdin.flush()

        def request(identifier, method, params):
            send({"jsonrpc": "2.0", "id": identifier, "method": method, "params": params})
            deadline = time.monotonic() + 30
            while True:
                message = messages.get(timeout=max(0.01, deadline - time.monotonic()))
                if isinstance(message, Exception):
                    raise message
                if message.get("id") != identifier:
                    continue
                if message.get("error"):
                    raise RuntimeError(f"MCP {method} returned an error: {message['error']}")
                return message["result"]

        try:
            initialized = request(1, "initialize", {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "oceanway-packaged-smoke", "version": "1"},
            })
            assert "tools" in initialized["capabilities"], "MCP tools capability missing"
            send({"jsonrpc": "2.0", "method": "notifications/initialized"})
            listed = request(2, "tools/list", {})
            assert {tool["name"] for tool in listed["tools"]} == TOOLS
            generate = next(tool for tool in listed["tools"] if tool["name"] == "generate_images")
            properties = generate["inputSchema"]["properties"]
            assert properties["count"]["default"] == 1
            assert properties["model"].get("default") in (None, ""), "Hardcoded image model"
            process.stdin.close()
            assert process.wait(timeout=15) == 0, "MCP did not exit cleanly after stdin EOF"
            for reader in readers:
                reader.join(timeout=2)
            assert not any(reader.is_alive() for reader in readers), "MCP pipe reader did not finish"
            assert FIXTURE_KEY not in "".join(transcript + errors), "Fixture key leaked to stdio"
            assert snapshot(home) == before, "Discovery changed application data"
            assert not list(cwd.iterdir()), "Discovery created files in the working directory"
            print(json.dumps({
                "ok": True,
                "executable": str(executable),
                "checks": ["initialize", "four-tools", "EOF-exit", "unicode-space-home",
                           "unrelated-cwd", "no-runtime-PATH", "app-data-unchanged"],
                "imageRequests": 0,
                "note": "Discovery only; no tools/call or HTTP generation request was made.",
            }, ensure_ascii=False))
        finally:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=10)
            for pipe in (process.stdin, process.stdout, process.stderr):
                if pipe and not pipe.closed:
                    pipe.close()
            for reader in readers:
                reader.join(timeout=2)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--executable", required=True, type=Path)
    options = parser.parse_args()
    check(options.executable)
