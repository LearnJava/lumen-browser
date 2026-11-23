# -*- coding: utf-8 -*-
"""Чистый wall-clock прогон без LUMEN_PROFILE_TREE: время до readyState=complete."""
import sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
sys.stdout.reconfigure(encoding="utf-8")
import perf_audit as pa

REPO = Path(__file__).resolve().parent.parent.parent
OUT = REPO / ".tmp" / "wall-run"
OUT.mkdir(parents=True, exist_ok=True)
url = sys.argv[1] if len(sys.argv) > 1 else "https://habr.com/ru/"
wait_s = float(sys.argv[2]) if len(sys.argv) > 2 else 120.0

br = pa.LiveBrowser(REPO / "target/dev-release/lumen.exe", OUT, timeout=200, tag="wall")
mcp = br.mcp
mcp.tool("new_tab", {"url": url})
t0 = time.monotonic()
complete_at = None
interactive_at = None
while time.monotonic() - t0 < wait_s:
    try:
        r = mcp.tool("eval", {"code": "document.readyState", "timeout_ms": 8000})
        s = str(r)
        if interactive_at is None and "interactive" in s:
            interactive_at = time.monotonic() - t0
        if "complete" in s:
            complete_at = time.monotonic() - t0
            break
    except Exception:
        pass
    time.sleep(0.5)
print(f"interactive={interactive_at} complete={complete_at}")
try:
    r = mcp.tool("eval", {"code": "JSON.stringify({dom: performance.timing.domContentLoadedEventEnd - performance.timing.navigationStart, load: performance.timing.loadEventEnd - performance.timing.navigationStart})", "timeout_ms": 8000})
    print("timing:", r)
except Exception as e:
    print("timing err", e)
try: br.proc.terminate()
except Exception: pass
time.sleep(1)
