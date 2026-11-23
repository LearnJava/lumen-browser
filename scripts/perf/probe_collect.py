# -*- coding: utf-8 -*-
import sys, time, json
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
sys.stdout.reconfigure(encoding="utf-8")
import perf_audit as pa
REPO = Path(__file__).resolve().parent.parent.parent
OUT = REPO / ".tmp" / "prof-run"; OUT.mkdir(parents=True, exist_ok=True)
pa.EXTRA_ENV["LUMEN_PROFILE_TREE"]="1"
pa.EXTRA_ENV["LUMEN_FRAME_LOG"]="1"
pa.EXTRA_ENV["LUMEN_PROFILE_DETAIL"]="1"
br = pa.LiveBrowser(REPO / "target/dev-release/lumen.exe", OUT, timeout=320, tag="collect")
mcp = br.mcp
mcp.tool("new_tab", {"url": "https://habr.com/ru/"})
t0 = time.monotonic()
COLLECT = open(Path(__file__).resolve().parent / "habr_collect.js", encoding="utf-8").read()
while time.monotonic() - t0 < 240:
    time.sleep(5)
    try:
        r = mcp.tool("eval", {"code": COLLECT, "timeout_ms": 15000})
        print("T", round(time.monotonic()-t0), type(r).__name__, str(r)[:200])
        if isinstance(r, dict) and r.get("success"):
            inner = r.get("result")
            print("inner head:", str(inner)[:150])
            try:
                data = json.loads(inner)
                if isinstance(data, str): data = json.loads(data)
                rs = data.get("rs") if isinstance(data, dict) else None
                print("PARSED rs=", rs, "res=", data.get("count") if isinstance(data, dict) else "?")
                if rs == "complete":
                    (REPO / ".tmp" / "habr_data.json").write_text(json.dumps(data, ensure_ascii=False), encoding="utf-8")
                    break
            except Exception as e:
                print("parse err:", e)
    except Exception as e:
        print("T", round(time.monotonic()-t0), "ERR", str(e)[:100])
try: br.proc.terminate()
except Exception: pass
