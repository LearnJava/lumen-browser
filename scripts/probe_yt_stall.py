import json, os, socket, subprocess, sys, time
from pathlib import Path

root = Path(__file__).resolve().parent.parent
exe = Path(sys.argv[1]) if len(sys.argv) > 1 else root / "target/dev-release/lumen.exe"
dur = int(sys.argv[2]) if len(sys.argv) > 2 else 120
url = sys.argv[3] if len(sys.argv) > 3 else "https://www.youtube.com/"
tag = sys.argv[4] if len(sys.argv) > 4 else "a"
log_path = root / ".tmp" / f"yt.{tag}.stderr.log"
s = socket.socket(); s.bind(("127.0.0.1", 0)); port = s.getsockname()[1]; s.close()
env = os.environ.copy()
env["LUMEN_NO_ADBLOCK"] = "1"
env.setdefault("LUMEN_JS_STALL_SAMPLE_MS", "500")
env.setdefault("LUMEN_FRAME_LOG", "1")
log = log_path.open("wb")
proc = subprocess.Popen([str(exe)] + os.environ.get("LUMEN_PROBE_ARGS", "").split() + ["--mcp-live-port", str(port), "--maximized", "about:blank"],
                        stdout=subprocess.DEVNULL, stderr=log, cwd=str(root), env=env)
for _ in range(300):
    try:
        sock = socket.create_connection(("127.0.0.1", port), timeout=5); break
    except OSError:
        time.sleep(0.1)
rd = sock.makefile("r", encoding="utf-8", newline="\n")
sock.settimeout(120)
n = 0
def call(method, params):
    global n
    n += 1
    sock.sendall((json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params}) + "\n").encode())
    return json.loads(rd.readline())
tok = None
for _ in range(200):
    for line in open(log_path, encoding="utf-8", errors="replace"):
        if line.startswith("[mcp] token: "):
            tok = line.strip()[13:]
    if tok: break
    time.sleep(0.1)
call("initialize", {"token": tok})
def tool(name, a): return call("tools/call", {"name": name, "arguments": a})
t0 = time.monotonic()
r = tool("navigate", {"url": url})
print("navigate", time.monotonic() - t0, str(r)[:200], flush=True)
while time.monotonic() - t0 < dur:
    t1 = time.monotonic()
    r = tool("eval", {"code": "document.readyState + ' ' + document.querySelectorAll('*').length", "timeout_ms": 5000})
    txt = json.dumps(r, ensure_ascii=False)[:200]
    print(f"{t1 - t0:6.1f}s eval {time.monotonic() - t1:5.2f}s {txt}", flush=True)
    time.sleep(3)
proc.kill()
