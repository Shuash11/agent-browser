"""Probe text-based page tools and DOM scrolling."""
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


send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
      "params": {"protocolVersion": "2025-11-25", "capabilities": {},
                 "clientInfo": {"name": "probe", "version": "1"}}})
print("init:", "ok" if "result" in wait(1) else "failed")
send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})

call(9, "browser_focus_tab", {"index": 1})
time.sleep(0.3)

for i, (name, args) in enumerate([
    ("browser_page_text", {}),
    ("browser_page_elements", {}),
    ("browser_scroll_dom", {"dx": 0, "dy": 200}),
    ("browser_scroll_into_view", {"text": "Button B"}),
], start=2):
    r = call(i, name, args)
    text = r.get("result", {}).get("content", [{}])[0].get("text", "")
    print(name, "->", text[:260])

p.terminate()
