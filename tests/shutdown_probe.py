"""Verify MCP server exits 0 when the client disconnects."""
import json, subprocess, time
from pathlib import Path

EXE = Path(__file__).resolve().parents[1] / "target" / "release" / "agent-browser.exe"
p = subprocess.Popen([str(EXE), "mcp"], stdin=subprocess.PIPE,
                     stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
p.stdin.write(json.dumps({
    "jsonrpc": "2.0", "id": 1, "method": "initialize",
    "params": {"protocolVersion": "2025-11-25", "capabilities": {},
               "clientInfo": {"name": "probe", "version": "1"}},
}) + "\n")
p.stdin.flush()
line = p.stdout.readline()
print("initialize reply:", line[:120])
p.stdin.close()
try:
    code = p.wait(timeout=8)
except subprocess.TimeoutExpired:
    p.terminate()
    code = p.wait()
print("exit code:", code)
