# -*- coding: utf-8 -*-
"""Профиль загрузки одного сайта: LUMEN_PROFILE_TREE=1, живое окно, stderr->tree."""
import re, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
sys.stdout.reconfigure(encoding="utf-8")
import perf_audit as pa

REPO = Path(__file__).resolve().parent.parent.parent
OUT = REPO / ".tmp" / "prof-run"
OUT.mkdir(parents=True, exist_ok=True)
url = sys.argv[1] if len(sys.argv) > 1 else "https://github.com/"
wait_s = float(sys.argv[2]) if len(sys.argv) > 2 else 75.0

pa.EXTRA_ENV["LUMEN_PROFILE_TREE"]="1"
pa.EXTRA_ENV["LUMEN_FRAME_LOG"]="1"
pa.EXTRA_ENV["LUMEN_PROFILE_DETAIL"]="1"
br = pa.LiveBrowser(REPO / "target/dev-release/lumen.exe", OUT, timeout=180, tag="prof")
mcp = br.mcp
mcp.tool("new_tab", {"url": url})
t0 = time.monotonic()
# ждём ready или таймаут
while time.monotonic() - t0 < wait_s:
    try:
        r = mcp.tool("eval", {"code": "document.readyState", "timeout_ms": 8000})
        if "complete" in str(r):
            break
    except Exception:
        pass
    time.sleep(1.0)
print(f"waited {time.monotonic()-t0:.0f}s")
time.sleep(3)
try: br.proc.terminate()
except Exception: pass
time.sleep(2)

# агрегируем [profile]-строки: имя -> суммарное время (все строки, не только root)
agg = {}
for lg in OUT.glob("live.stderr.prof.*.log"):
    txt = lg.read_bytes().decode("utf-8", "replace")
    for m in re.finditer(r"\[profile\]\s+([\d.]+)ms\s+(.*)", txt):
        ms = float(m.group(1)); name = m.group(2).strip()
        agg[name] = agg.get(name, 0.0) + ms
print("== TOP scopes by total ms ==")
for name, ms in sorted(agg.items(), key=lambda kv: -kv[1])[:25]:
    print(f"{ms:10.0f}  {name[:100]}")
