"""Verify cached layout IDs and ID-based click/type tools."""
import json, queue, subprocess, threading, time
from pathlib import Path

EXE = Path(__file__).resolve().parents[1] / "target" / "debug" / "agent-browser.exe"
p = subprocess.Popen([str(EXE), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.PIPE, text=True, bufsize=1)
q = queue.Queue()
threading.Thread(target=lambda: [q.put(l) for l in p.stdout], daemon=True).start()


def send(obj):
    p.stdin.write(json.dumps(obj) + "\n")
    p.stdin.flush()


def wait(i):
    end = time.time() + 15
    while time.time() < end:
        try:
            m = json.loads(q.get(timeout=0.3))
        except queue.Empty:
            continue
        if m.get("id") == i:
            return m
    return {"timeout": True}


def call(i, name, args=None):
    send({"jsonrpc": "2.0", "id": i, "method": "tools/call",
          "params": {"name": name, "arguments": args or {}}})
    return wait(i)


def text(r):
    return r.get("result", {}).get("content", [{}])[0].get("text", "")


send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                 "clientInfo": {"name": "probe", "version": "1"}}})
wait(1)
send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
call(2, "browser_focus_tab", {"index": 1})
time.sleep(0.4)

layout = text(call(3, "browser_layout", {"refresh": True}))
print("layout:", layout[:300])
cached = text(call(4, "browser_layout", {}))
print("cached same:", cached == layout)

print("bad id:", text(call(5, "browser_click_id", {"id": "nope"}))[:160])

# e1 is usually Button A in the fixture.
print("click e1:", text(call(6, "browser_click_id", {"id": "e1"}))[:160])

# Find the text input ID from the cached layout and type into it.
try:
    data = json.loads(cached)
    target = next(e["id"] for e in data["elements"] if e["tag"] == "INPUT")
    print("type into", target, ":", text(call(
        7, "browser_type_id", {"id": target, "text": "HELLO"}))[:160])
except Exception as exc:
    print("type skipped:", exc)

p.terminate()
