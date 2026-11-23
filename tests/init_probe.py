"""Probe which initialize payloads the MCP server accepts."""
import json, subprocess, threading, queue, time
from pathlib import Path

EXE = Path(__file__).resolve().parents[1] / "target" / "release" / "agent-browser.exe"

VARIANTS = [
    {"jsonrpc": "2.0", "id": 1, "method": "initialize",
     "params": {"protocolVersion": "2026-07-28", "capabilities": {},
                "clientInfo": {"name": "smoke", "version": "1"}}},
    {"jsonrpc": "2.0", "id": 1, "method": "initialize",
     "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "opencode", "version": "1"}}},
    {"jsonrpc": "2.0", "id": 1, "method": "initialize",
     "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                "clientInfo": {"name": "x", "version": "1"},
                "rootUri": None}},
]

for i, init in enumerate(VARIANTS):
    p = subprocess.Popen([str(EXE), "mcp"], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                         text=True, bufsize=1)
    q = queue.Queue()
    threading.Thread(target=lambda: [q.put(l) for l in p.stdout], daemon=True).start()
    p.stdin.write(json.dumps(init) + "\n")
    p.stdin.flush()
    got = None
    deadline = time.time() + 4
    while time.time() < deadline:
        try:
            line = q.get(timeout=0.2)
        except queue.Empty:
            if p.poll() is not None:
                break
            continue
        got = line.strip()
        break
    print(i, "alive=", p.poll() is None, "reply=", (got or "")[:160])
    p.terminate()
