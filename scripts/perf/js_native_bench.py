# -*- coding: utf-8 -*-
"""Микробенч JS↔нэйтив границы на живой странице: сколько ns стоит один вызов.

Грузит URL, ждёт complete, затем eval'ит серии `performance.now()`-замеров:
для каждого нэйтива — прогрев + N итераций, печатает ns/вызов и что внутри
(лок doc, конверсии, вложенный flush).
"""
import sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
sys.stdout.reconfigure(encoding="utf-8")
import perf_audit as pa

REPO = Path(__file__).resolve().parent.parent.parent
OUT = REPO / ".tmp" / "native-bench"
OUT.mkdir(parents=True, exist_ok=True)
url = sys.argv[1] if len(sys.argv) > 1 else "https://habr.com/"

pa.EXTRA_ENV["LUMEN_NO_ADBLOCK"] = "1"
br = pa.LiveBrowser(REPO / "target/dev-release/lumen.exe", OUT, timeout=180, tag="nb")
mcp = br.mcp
mcp.tool("new_tab", {"url": url})
t0 = time.monotonic()
while time.monotonic() - t0 < 80:
    try:
        r = mcp.tool("eval", {"code": "document.readyState", "timeout_ms": 8000})
        if "complete" in str(r):
            break
    except Exception:
        pass
    time.sleep(1.0)
print(f"waited {time.monotonic()-t0:.0f}s")
time.sleep(2)

BENCH = r"""
(() => {
  const out = {};
  const bench = (name, setup, fn, n) => {
    setup(); fn(); fn(); // прогрев
    const t = performance.now();
    for (let i = 0; i < n; i++) fn();
    const dt = performance.now() - t;
    out[name] = `${(dt * 1e6 / n).toFixed(0)} ns ×${n}  (${dt.toFixed(1)}ms)`;
  };
  // корпус узлов для навигационных нэйтивов
  const nodes = [];
  const w = document.createTreeWalker(document.body, NodeFilter.SHOW_ELEMENT);
  let c; while ((c = w.nextNode()) && nodes.length < 5000) nodes.push(c);
  let k = 0;
  const next = () => nodes[k++ % nodes.length];

  bench("parentNode", () => {}, () => next().parentNode, 200000);
  bench("nextSibling", () => {}, () => next().nextSibling, 200000);
  bench("nextElementSibling", () => {}, () => next().nextElementSibling, 200000);
  bench("firstChild", () => {}, () => next().firstChild, 200000);
  bench("parentElement", () => {}, () => next().parentElement, 200000);
  bench("childElementCount", () => {}, () => next().childElementCount, 100000);
  bench("tree walk 2k nodes", () => { window.__tw = 0; }, () => {
    // firstChild -> nextSibling обход всего body — паттерн reconciler'ов
    let n = document.body.firstChild, guard = 0;
    while (n && guard++ < 4000) { window.__tw++; n = n.nextSibling; }
  }, 20);
  bench("getAttribute(class)", () => {}, () => next().getAttribute("class"), 100000);
  bench("children.length", () => {}, () => next().children.length, 100000);
  bench("textContent read", () => {}, () => { let s=0; const e=next(); s+=e.textContent.length; return s; }, 50000);
  bench("appendChild+remove", () => {
    window.__benchHost = document.createElement("div");
    document.body.appendChild(window.__benchHost);
  }, () => {
    const d = document.createElement("div");
    window.__benchHost.appendChild(d);
    window.__benchHost.removeChild(d);
  }, 20000);
  bench("getBoundingClientRect", () => {}, () => next().getBoundingClientRect(), 20000);
  bench("querySelector(div)", () => {}, () => document.querySelector("div"), 20000);
  bench("getComputedStyle", () => {}, () => getComputedStyle(next()).color, 10000);
  bench("classList.add", () => {}, () => { const e = next(); e.classList.add("x-bench"); e.classList.remove("x-bench"); }, 20000);
  bench("createElement", () => {}, () => document.createElement("div"), 20000);
  bench("createTextNode", () => {}, () => document.createTextNode("x"), 20000);
  bench("appendChild only", () => {
    window.__benchHost2 = document.createElement("div");
    document.body.appendChild(window.__benchHost2);
  }, () => {
    const d = document.createElement("div");
    window.__benchHost2.appendChild(d);
  }, 20000);
  bench("innerHTML read", () => {}, () => next().innerHTML.length, 20000);
  return JSON.stringify(out, null, 1);
})()
"""
r = mcp.tool("eval", {"code": BENCH, "timeout_ms": 300000})
print(r)
try:
    br.proc.terminate()
except Exception:
    pass
