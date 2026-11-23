"""Smoke test: drive the GUI through the MCP stdio server."""
import json
import queue
import subprocess
import sys
import threading
import time
from pathlib import Path

EXE = Path(__file__).resolve().parents[1] / "target" / "debug" / "agent-browser.exe"


def main() -> None:
    p = subprocess.Popen(
        [str(EXE), "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
        stderr=subprocess.PIPE, text=True, bufsize=1,
    )
    q: queue.Queue[str] = queue.Queue()

    def reader() -> None:
        for line in p.stdout:
            q.put(line)
    threading.Thread(target=reader, daemon=True).start()

    def send(obj: dict) -> None:
        p.stdin.write(json.dumps(obj) + "\n")
        p.stdin.flush()

    def wait_for(req_id: int) -> dict:
        deadline = time.time() + 40
        while time.time() < deadline:
            try:
                line = q.get(timeout=0.3)
            except queue.Empty:
                continue
            try:
                msg = json.loads(line)
            except json.JSONDecodeError:
                continue
            if msg.get("id") == req_id:
                return msg
        raise TimeoutError(f"no reply for id {req_id}")

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize",
          "params": {"protocolVersion": "2026-07-28", "capabilities": {},
                     "clientInfo": {"name": "smoke", "version": "1"}}})
    print("initialize:", json.dumps(wait_for(1))[:200])
    send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})

    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
    tools = wait_for(2)
    names = [t["name"] for t in tools.get("result", {}).get("tools", [])]
    print("tools:", names)

    def call(name: str, args: dict, req_id: int) -> dict:
        send({"jsonrpc": "2.0", "id": req_id, "method": "tools/call",
              "params": {"name": name, "arguments": args}})
        return wait_for(req_id)

    vp_resp = call("browser_get_viewport", {}, 3)
    print("viewport:", vp_resp)
    if "result" not in vp_resp:
        print("viewport error:", vp_resp)
        return
    vp = json.loads(vp_resp["result"]["content"][0]["text"])["viewport"]
    # Button A centre in the fixture, relative to the content viewport.
    bx, by = vp["x"] + 100, vp["y"] + 221
    shot = str(Path(__file__).resolve().parents[1] / "analysis_mcp_before.png")
    print("screenshot:", call("browser_screenshot", {"path": shot}, 4))
    print("click:", call("browser_click", {"x": bx, "y": by, "button": "left"}, 5))
    time.sleep(0.4)
    shot2 = str(Path(__file__).resolve().parents[1] / "analysis_mcp_after.png")
    print("screenshot2:", call("browser_screenshot", {"path": shot2}, 6))
    print("click text:", call("browser_click", {"x": vp["x"] + 440, "y": vp["y"] + 221, "button": "left"}, 7))
    print("type:", call("browser_type", {"text": "HELLO"}, 8))
    time.sleep(0.3)
    shot3 = str(Path(__file__).resolve().parents[1] / "analysis_mcp_typed.png")
    print("screenshot3:", call("browser_screenshot", {"path": shot3}, 9))
    print("dock bottom:", call("browser_dock", {"mode": "bottom"}, 10))
    time.sleep(0.5)
    shot4 = str(Path(__file__).resolve().parents[1] / "analysis_mcp_bottom.png")
    print("bottom shot:", call("browser_screenshot", {"path": shot4}, 11))
    print("dock left:", call("browser_dock", {"mode": "left"}, 12))
    p.terminate()


if __name__ == "__main__":
    main()
