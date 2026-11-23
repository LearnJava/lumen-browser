# -*- coding: utf-8 -*-
"""Один сайт в Lumen и в Chrome: время загрузки, сеть, консоль, ошибки — холодный кэш, без прокси.

    python scripts/perf/site_ab.py --url https://okko.tv/ --runs 5 [--arms lumen,chrome] [--settle 15]
        [--challenge-selector 'js-challenge-loader'] [--env LUMEN_JS_STALL_SAMPLE_MS=250] [--chrome-arg ...]

Оба браузера идут напрямую: Lumen — `LUMEN_NO_SYSTEM_PROXY=1`, Chrome — `--no-proxy-server` (включённый
системный прокси Windows иначе забирает трафик у обоих, BUG-1803). Прогоны чередуются (L, C, L, C…) — шум
машины превышает большинство эффектов (docs/automation.md §Quick chooser). Один прогон — свежий процесс и
холодный профиль: у Lumen жёсткая ссылка на exe в пустом каталоге даёт чистый `data/` (`<каталог exe>/data`:
HTTP-кэш, cookie, хранилища — приём scripts/compat_ab_lumen.py), у Chrome — свежий `--user-data-dir`.

Сигналы времени считаются от t0 (навигация); страница опрашивается каждые 250 мс одним и тем же JS в обоих
браузерах: `t_load` — первый `readyState == "complete"`; `t_ch_gone` — сошёл challenge-плейсхолдер (селектор
`--challenge-selector`); `t_app_load` — `complete` документа без плейсхолдера; `t_content` — загружено
`--content-imgs` картинок. Итоговые числа страницы — Navigation Timing, Paint, Resource Timing, longtask, LCP.
Сеть и консоль: Chrome — CDP (`Network.*`, `Runtime.exceptionThrown`, `Runtime.consoleAPICalled`,
`Log.entryAdded`), Lumen — `resource://network`, `resource://console` и stderr. Кадр — снимок экрана
(PIL ImageGrab), одинаковый для обоих.

Результаты: `<out>/results.json` (все прогоны), `<out>/r<NN>-<arm>/` (stderr-лог, `screen.jpg`).
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent
sys.path.insert(0, str(REPO / "scripts"))
sys.stdout.reconfigure(encoding="utf-8")
import perf_audit as pa  # noqa: E402

CHROME = Path(os.environ.get("CHROME_EXE", r"C:\Program Files\Google\Chrome\Application\chrome.exe"))

PROBE_JS = r"""(() => {
  const d = document;
  let ok = 0;
  for (const i of d.images) if (i.complete && i.naturalWidth > 0) ok++;
  let ch = false;
  try { ch = !!d.querySelector(__CH__); } catch (e) {}
  let to = null;
  try { to = Math.round(performance.timeOrigin); } catch (e) {}
  return JSON.stringify({ rs: d.readyState, ch, ok, n: d.getElementsByTagName('*').length,
                          href: String(location.href).slice(0, 160), to });
})()"""

FINAL_JS = r"""(() => {
  const o = {};
  const nav = performance.getEntriesByType('navigation')[0];
  o.nav = nav ? Object.fromEntries([
    'fetchStart', 'domainLookupStart', 'domainLookupEnd', 'connectStart', 'secureConnectionStart',
    'connectEnd', 'requestStart', 'responseStart', 'responseEnd', 'domInteractive',
    'domContentLoadedEventEnd', 'domComplete', 'loadEventEnd', 'transferSize', 'encodedBodySize',
    'decodedBodySize', 'redirectCount', 'type',
  ].map(k => [k, typeof nav[k] === 'number' ? Math.round(nav[k] * 10) / 10 : nav[k]])) : null;
  try {
    o.paint = Object.fromEntries(performance.getEntriesByType('paint').map(p => [p.name, Math.round(p.startTime)]));
  } catch (e) { o.paint_err = String(e); }
  try {
    const rs = performance.getEntriesByType('resource'); const hosts = {}; let bytes = 0;
    const slow = rs.map(r => [Math.round(r.startTime), Math.round(r.duration), Math.round(r.transferSize || 0),
                               String(r.name).slice(0, 140)]).sort((a, b) => b[1] - a[1]);
    for (const r of rs) {
      try { const h = new URL(r.name).host; hosts[h] = (hosts[h] || 0) + 1; } catch (e) {}
      bytes += (r.transferSize || 0);
    }
    o.res = { n: rs.length, bytes, hosts, slow: slow.slice(0, 25) };
  } catch (e) { o.res_err = String(e); }
  try {
    const w = window; w.__ablt = []; w.__ablcp = [];
    new PerformanceObserver(l => {
      for (const e of l.getEntries()) w.__ablt.push([Math.round(e.startTime), Math.round(e.duration)]);
    }).observe({ type: 'longtask', buffered: true });
    new PerformanceObserver(l => {
      for (const e of l.getEntries()) w.__ablcp.push([Math.round(e.startTime), e.size, e.url ? String(e.url).slice(0, 120) : null]);
    }).observe({ type: 'largest-contentful-paint', buffered: true });
  } catch (e) { o.obs_err = String(e); }
  return JSON.stringify(o);
})()"""

READ_ASYNC_JS = r"""JSON.stringify({ lt: (window.__ablt || []).slice(-80), lcp: (window.__ablcp || []).slice(-10) })"""

CHROME_INIT_JS = r"""(() => {
  const P = window.__ab = { lt: [], lcp: [], errs: [] };
  try {
    new PerformanceObserver(l => { for (const e of l.getEntries()) P.lt.push([Math.round(e.startTime), Math.round(e.duration)]); })
      .observe({ type: 'longtask', buffered: true });
  } catch (e) { P.ltErr = String(e); }
  window.addEventListener('error', ev => P.errs.push(String(ev.message).slice(0, 200) + ' @' + (ev.filename || '') + ':' + ev.lineno), true);
  window.addEventListener('unhandledrejection', ev => P.errs.push('rejection: ' + String(ev.reason).slice(0, 200)));
})();"""


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def _unwrap(r) -> dict | None:
    """eval возвращает JSON-сериализацию значения; строку JSON — развернуть (до 3 раз)."""
    val = r.get("result") if isinstance(r, dict) else r
    for _ in range(3):
        if isinstance(val, str):
            try:
                val = json.loads(val)
            except json.JSONDecodeError:
                break
        else:
            break
    return val if isinstance(val, dict) else None


def grab(path: Path) -> None:
    """Кадр с экрана: окно браузера развернуто и на переднем плане — одинаково для обоих."""
    try:
        from PIL import ImageGrab
        ImageGrab.grab(all_screens=True).convert("RGB").save(str(path), "JPEG", quality=82)
    except Exception as e:  # noqa: BLE001 — скриншот не условие замера
        path.with_suffix(".err").write_text(str(e)[:200], encoding="utf-8")


def proc_stats(pid: int) -> dict:
    """CPU дерева процессов (с) и топ потоков главного по CPU — где сгорает время (psutil, Windows)."""
    try:
        import ctypes

        import psutil
    except ImportError:
        return {}
    try:
        root = psutil.Process(pid)
        procs = [root, *root.children(recursive=True)]
        cpu = 0.0
        rss = 0
        for p in procs:
            try:
                t = p.cpu_times()
                cpu += t.user + t.system
                rss += p.memory_info().rss
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass
        k32 = ctypes.windll.kernel32
        ntdll = ctypes.windll.ntdll
        k32.OpenThread.restype = ctypes.c_void_p
        k32.GetThreadDescription.argtypes = [ctypes.c_void_p, ctypes.POINTER(ctypes.c_void_p)]
        k32.LocalFree.argtypes = [ctypes.c_void_p]
        ntdll.NtQueryInformationThread.argtypes = [
            ctypes.c_void_p, ctypes.c_ulong, ctypes.c_void_p, ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong)
        ]
        ntdll.NtQueryInformationThread.restype = ctypes.c_long
        # База образа в целевом процессе: адрес старта потока → RVA → символ по PDB.
        base = None
        try:
            base = min(int(m.addr, 16) for m in root.memory_maps(grouped=False)
                       if m.path and m.path.lower().endswith(".exe"))
        except (psutil.AccessDenied, ValueError, OSError):
            base = None
        threads = []
        for th in root.threads():
            name = None
            start_rva = None
            # THREAD_QUERY_LIMITED_INFORMATION | THREAD_QUERY_INFORMATION (адрес старта требует второго)
            h = k32.OpenThread(0x0800 | 0x0040, False, th.id)
            if h:
                desc = ctypes.c_void_p()
                if k32.GetThreadDescription(h, ctypes.byref(desc)) and desc.value:
                    name = ctypes.wstring_at(desc.value)
                    k32.LocalFree(desc.value)
                start = ctypes.c_void_p()
                ret_len = ctypes.c_ulong()
                # ThreadQuerySetWin32StartAddress = 9
                if ntdll.NtQueryInformationThread(h, 9, ctypes.byref(start), ctypes.sizeof(start),
                                                  ctypes.byref(ret_len)) == 0 and start.value:
                    start_rva = hex(start.value - base) if base else hex(start.value)
                k32.CloseHandle(h)
            threads.append([name or f"tid {th.id}", round(th.user_time + th.system_time, 2), start_rva])
        threads.sort(key=lambda x: -x[1])
        return {"tree_cpu_s": round(cpu, 2), "tree_rss_mb": round(rss / 1048576),
                "tree_procs": len(procs), "threads_top": threads[:16]}
    except Exception as e:  # noqa: BLE001 — статистика не условие замера
        return {"proc_stats_error": str(e)[:120]}


def metrics(rec: dict, a) -> dict:
    """Время сигналов (с от t0) по опросам; выборка: [t, readyState, challenge, imgs_ok, nodes, href]."""
    S = [s for s in rec.get("samples", []) if s[1] is not None]
    first = lambda pred: next((s[0] for s in S if pred(s)), None)  # noqa: E731
    ch_seen = any(s[2] for s in S)
    m = {"challenge": ch_seen, "t_load": first(lambda s: s[1] == "complete")}
    m["t_ch_gone"] = first(lambda s: not s[2]) if ch_seen else None
    m["t_app_load"] = first(lambda s: s[1] == "complete" and not s[2]) if ch_seen else m["t_load"]
    m["t_content"] = first(lambda s: s[1] == "complete" and not s[2] and (s[3] or 0) >= a.content_imgs)
    fin = rec.get("final") or {}
    nav = fin.get("nav") or {}
    m["doc_dcl_ms"] = nav.get("domContentLoadedEventEnd")
    m["doc_load_ms"] = nav.get("loadEventEnd")
    m["doc_resp_ms"] = nav.get("responseEnd")
    m["fp_ms"] = (fin.get("paint") or {}).get("first-paint")
    lcp = ((rec.get("async") or {}).get("lcp") or [])
    m["lcp_ms"] = lcp[-1][0] if lcp else None
    lt = ((rec.get("async") or {}).get("lt") or [])
    m["longtask_ms"] = sum(x[1] for x in lt)
    m["longtask_n"] = len(lt)
    m["res_n"] = (fin.get("res") or {}).get("n")
    m["obs_err"] = fin.get("obs_err")
    return m


def lumen_stderr(log_path: Path) -> dict:
    try:
        lines = log_path.read_bytes().decode("utf-8", errors="replace").splitlines()
    except OSError:
        return {}
    net = [ln.strip() for ln in lines if ln.startswith(("→", "←", "✗"))]
    errs = list(dict.fromkeys(ln.strip()[:240] for ln in lines
                              if not ln.startswith(("→", "←")) and pa.ERROR_RE.search(ln)))
    return {"net_lines": net[:1500], "net_reqs": sum(1 for ln in net if ln.startswith("→")),
            "net_fail": sum(1 for ln in net if "✗" in ln), "stderr_lines": len(lines),
            "stderr_errors": errs[:80]}


def run_lumen(k: int, a, run_dir: Path, bin_dir: Path, probe_js: str) -> dict:
    exe = bin_dir / "lumen.exe"
    bin_dir.mkdir(parents=True, exist_ok=True)
    if not exe.exists():
        os.link(a.exe, exe)
    shutil.rmtree(bin_dir / "data", ignore_errors=True)  # холодный профиль: только наш каталог
    pa.EXTRA_ENV.update(a.env)
    pa.EXTRA_ENV["LUMEN_NO_SYSTEM_PROXY"] = "1"
    rec = {"arm": "lumen", "run": k, "env": dict(a.env)}
    br = None
    t0 = time.monotonic()
    try:
        br = pa.LiveBrowser(exe, run_dir, timeout=a.total + 30, tag=f"r{k:02d}")
        for mon in (br.ram, br.os):
            try:
                mon.begin_site()
            except Exception:  # noqa: BLE001
                pass
        rec["spawn_s"] = br.spawn_s
        t0 = time.monotonic()
        try:
            br.mcp.tool("new_tab", {"url": a.url})
        except RuntimeError:
            br.mcp.tool("navigate", {"url": a.url})
        rec["nav_ack_s"] = round(time.monotonic() - t0, 3)

        def ev(code: str, ms: int):
            return _unwrap(br.mcp.tool("eval", {"code": code, "timeout_ms": ms}))

        rec["samples"], rec["poll"] = poll(lambda: ev(probe_js, 8000), t0, a)
        rec["final"] = ev(FINAL_JS, 20000)
        time.sleep(0.6)
        rec["async"] = ev(READ_ASYNC_JS, 20000)
        rec["console_raw"] = [c.get("text", "") for c in br.mcp.resource("resource://console")]
        rec["network_raw"] = [c.get("text", "") for c in br.mcp.resource("resource://network")]
        grab(run_dir / "screen.jpg")
    except (RuntimeError, OSError, socket.timeout, json.JSONDecodeError) as e:
        rec["error"] = f"{type(e).__name__}: {str(e)[:240]}"
    finally:
        log_path = health = None
        if br is not None:
            log_path, health = br.log_path, br.health_log_path
            for mon in (br.ram, br.os):
                try:
                    rec.update(mon.site_stats())
                except Exception:  # noqa: BLE001
                    pass
            rec["proc"] = proc_stats(br.proc.pid)
            br.close()
        if health is not None:
            try:
                health.unlink()
            except OSError:
                pass
        if log_path is not None:
            rec["stderr"] = lumen_stderr(log_path)
    return rec


def poll(eval_once, t0: float, a) -> tuple[list, dict]:
    """Опрос страницы до `settle` секунд после load без смены документа либо до `total`."""
    samples: list = []
    done_since = None
    last_href = None
    errors = 0
    last_err = None
    while True:
        t = round(time.monotonic() - t0, 3)
        if t >= a.total:
            break
        try:
            s = eval_once()
        except (RuntimeError, OSError, socket.timeout, json.JSONDecodeError) as e:
            s, errors, last_err = None, errors + 1, f"{type(e).__name__}: {str(e)[:160]}"
        if s is None or "rs" not in s:
            samples.append([t, None, None, None, None, None])
        else:
            if s["href"] != last_href:
                last_href, done_since = s["href"], None
            samples.append([t, s["rs"], bool(s["ch"]), s["ok"], s["n"], s["href"]])
            if s["rs"] == "complete":
                done_since = t if done_since is None else done_since
                if t - done_since >= a.settle:
                    break
        time.sleep(a.probe_ms / 1000)
    return samples, {"poll_errors": errors, "poll_last_err": last_err}


class Cdp:
    """Минимальный CDP-клиент по страничному websocket (`/json/list`), без сессий."""

    def __init__(self) -> None:
        self.events: list[dict] = []
        self.pending: dict = {}
        self._id = 0
        self.t0_epoch: float | None = None

    async def connect(self, url: str) -> None:
        import websockets
        self.ws = await websockets.connect(url, max_size=64 * 1024 * 1024)
        self.reader = asyncio.create_task(self._read())

    async def _read(self) -> None:
        try:
            async for raw in self.ws:
                m = json.loads(raw)
                if "id" in m:
                    f = self.pending.pop(m["id"], None)
                    if f is not None and not f.done():
                        f.set_result(m)
                else:
                    m["_t"] = round(time.time() - self.t0_epoch, 3) if self.t0_epoch else None
                    self.events.append(m)
        except Exception:  # noqa: BLE001 — сокет закрыт вместе с браузером
            pass

    async def send(self, method: str, params: dict | None = None) -> dict:
        self._id += 1
        f = asyncio.get_running_loop().create_future()
        self.pending[self._id] = f
        await self.ws.send(json.dumps({"id": self._id, "method": method, "params": params or {}}))
        r = await asyncio.wait_for(f, 60)
        if "error" in r:
            raise RuntimeError(f"{method}: {r['error']}")
        return r.get("result") or {}


def chrome_net(cdp: Cdp) -> tuple[list[dict], list[dict]]:
    """Сеть и консоль из событий CDP; времена — с от t0 (wallTime события либо момент прихода)."""
    reqs: dict[str, dict] = {}
    console: list[dict] = []
    off = 0.0
    for ev in cdp.events:
        m, p = ev.get("method"), ev.get("params") or {}
        if m == "Network.requestWillBeSent":
            r = reqs.setdefault(p["requestId"], {})
            if "wallTime" in p:
                off = p["wallTime"] - p["timestamp"]
                r["t_req"] = round(p["wallTime"] - cdp.t0_epoch, 3)
            req = p["request"]
            r.update(url=req["url"][:300], method=req["method"], type=p.get("type"),
                     init=(p.get("initiator") or {}).get("type"))
            if "redirectResponse" in p:
                r["redirect_from"] = p["redirectResponse"].get("status")
        elif m == "Network.responseReceived":
            resp = p["response"]
            r = reqs.setdefault(p["requestId"], {})
            r.update(status=resp.get("status"), mime=resp.get("mimeType"), proto=resp.get("protocol"),
                     cache=bool(resp.get("fromDiskCache") or resp.get("fromServiceWorker")),
                     ip=resp.get("remoteIPAddress"))
            if "wallTime" in p:
                r["t_resp"] = round(p["wallTime"] - cdp.t0_epoch, 3)
        elif m == "Network.loadingFinished":
            r = reqs.setdefault(p["requestId"], {})
            r["bytes"] = p.get("encodedDataLength")
            r["t_end"] = round(p["timestamp"] + off - cdp.t0_epoch, 3)
        elif m == "Network.loadingFailed":
            r = reqs.setdefault(p["requestId"], {})
            r.update(failed=p.get("errorText"), blocked=p.get("blockedReason"), canceled=p.get("canceled"))
        elif m == "Runtime.exceptionThrown":
            d = p["exceptionDetails"]
            desc = (d.get("exception") or {}).get("description") or d.get("text") or ""
            console.append({"k": "exception", "t": ev["_t"], "text": desc[:300],
                            "url": str(d.get("url") or "")[:160], "line": d.get("lineNumber")})
        elif m == "Runtime.consoleAPICalled" and p.get("type") in ("error", "assert", "warning"):
            text = " ".join(str(x.get("value", x.get("description", "")))[:200] for x in p.get("args", []))
            console.append({"k": "console." + p["type"], "t": ev["_t"], "text": text[:300]})
        elif m == "Log.entryAdded":
            e = p["entry"]
            if e.get("level") in ("error", "warning"):
                console.append({"k": "log." + e["level"], "src": e.get("source"), "t": ev["_t"],
                                "text": str(e.get("text"))[:300], "url": str(e.get("url") or "")[:160]})
    return list(reqs.values()), console


async def run_chrome(k: int, a, run_dir: Path, probe_js: str) -> dict:
    port = free_port()
    profile = Path(tempfile.mkdtemp(prefix=f"lumen-site-ab-{k}-"))
    rec = {"arm": "chrome", "run": k}
    proc = subprocess.Popen(
        [str(CHROME), f"--remote-debugging-port={port}", f"--user-data-dir={profile}", "--no-proxy-server",
         "--no-first-run", "--no-default-browser-check", "--start-maximized", *a.chrome_arg, "about:blank"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    cdp = Cdp()
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))  # loopback — мимо системного прокси
    try:
        t_spawn = time.monotonic()
        page = None
        for _ in range(600):
            if proc.poll() is not None:
                raise RuntimeError(f"chrome exited with code {proc.returncode}")
            try:
                targets = json.loads(opener.open(f"http://127.0.0.1:{port}/json/list", timeout=2).read())
                ver = json.loads(opener.open(f"http://127.0.0.1:{port}/json/version", timeout=2).read())
                page = next(t for t in targets if t.get("type") == "page")
                break
            except (OSError, StopIteration, json.JSONDecodeError):
                await asyncio.sleep(0.1)
        if page is None:
            raise RuntimeError("CDP-порт chrome не поднялся")
        await cdp.connect(page["webSocketDebuggerUrl"])
        rec.update(spawn_s=round(time.monotonic() - t_spawn, 2), browser=ver.get("Browser"))
        for dom in ("Page", "Network", "Runtime", "Log", "Performance"):
            await cdp.send(f"{dom}.enable")
        await cdp.send("Page.addScriptToEvaluateOnNewDocument", {"source": CHROME_INIT_JS})

        async def ev(code: str):
            r = await cdp.send("Runtime.evaluate", {"expression": code, "returnByValue": True})
            v = (r.get("result") or {}).get("value")
            return json.loads(v) if isinstance(v, str) else None

        cdp.t0_epoch = time.time()
        t0 = time.monotonic()
        await cdp.send("Page.navigate", {"url": a.url})
        rec["nav_ack_s"] = round(time.monotonic() - t0, 3)
        rec["samples"], rec["poll"] = await apoll(ev, probe_js, t0, a)
        rec["final"] = await ev(FINAL_JS)
        await asyncio.sleep(0.6)
        rec["async"] = await ev(READ_ASYNC_JS)
        try:
            pm = await cdp.send("Performance.getMetrics")
            rec["perf_metrics"] = {m["name"]: m["value"] for m in pm.get("metrics", [])
                                   if m["name"] in ("TaskDuration", "ScriptDuration", "LayoutDuration",
                                                    "RecalcStyleDuration", "JSHeapUsedSize", "Nodes",
                                                    "LayoutCount", "RecalcStyleCount")}
        except RuntimeError:
            pass
        rec["net"], rec["console"] = chrome_net(cdp)
        grab(run_dir / "screen.jpg")
    except (RuntimeError, OSError, json.JSONDecodeError) as e:
        rec["error"] = f"{type(e).__name__}: {str(e)[:240]}"
    finally:
        rec["proc"] = proc_stats(proc.pid)
        try:
            await cdp.send("Browser.close")
        except Exception:  # noqa: BLE001
            pass
        try:
            proc.wait(timeout=10)
        except subprocess.TimeoutExpired:
            subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)], capture_output=True)
        shutil.rmtree(profile, ignore_errors=True)  # наш временный профиль
    return rec


async def apoll(ev, probe_js: str, t0: float, a) -> tuple[list, dict]:
    samples: list = []
    done_since = None
    last_href = None
    errors = 0
    last_err = None
    while True:
        t = round(time.monotonic() - t0, 3)
        if t >= a.total:
            break
        try:
            s = await ev(probe_js)
        except (RuntimeError, OSError, json.JSONDecodeError) as e:
            s, errors, last_err = None, errors + 1, f"{type(e).__name__}: {str(e)[:160]}"
        if s is None or "rs" not in s:
            samples.append([t, None, None, None, None, None])
        else:
            if s["href"] != last_href:
                last_href, done_since = s["href"], None
            samples.append([t, s["rs"], bool(s["ch"]), s["ok"], s["n"], s["href"]])
            if s["rs"] == "complete":
                done_since = t if done_since is None else done_since
                if t - done_since >= a.settle:
                    break
        await asyncio.sleep(a.probe_ms / 1000)
    return samples, {"poll_errors": errors, "poll_last_err": last_err}


def fmt(v, nd: int = 2, scale: float = 1.0) -> str:
    return "-" if v is None else f"{v / scale:.{nd}f}"


def main() -> None:
    ap_ = argparse.ArgumentParser(description="Один сайт в Lumen и в Chrome: загрузка, сеть, консоль, ошибки")
    ap_.add_argument("--url", required=True)
    ap_.add_argument("--runs", type=int, default=3, help="прогонов на браузер")
    ap_.add_argument("--arms", default="lumen,chrome")
    ap_.add_argument("--settle", type=float, default=15.0, help="секунд после load без смены документа")
    ap_.add_argument("--total", type=float, default=90.0, help="потолок одного прогона, с")
    ap_.add_argument("--probe-ms", type=int, default=250, help="период опроса страницы, мс")
    ap_.add_argument("--content-imgs", type=int, default=10, help="сколько загруженных картинок = контент")
    ap_.add_argument("--challenge-selector", default="", help="CSS-селектор challenge-плейсхолдера на странице")
    ap_.add_argument("--exe", help="lumen.exe (иначе $LUMEN_EXE / target/*, как perf_audit)")
    ap_.add_argument("--env", action="append", default=[], metavar="KEY=VAL", help="в окружение lumen (повторяемый)")
    ap_.add_argument("--chrome-arg", action="append", default=[], metavar="FLAG")
    ap_.add_argument("--out", help="каталог результатов (иначе .tmp/site-ab/<штамп>)")
    a = ap_.parse_args()
    a.exe = (Path(a.exe) if a.exe else pa.find_exe(None)).resolve()
    a.env = dict(kv.split("=", 1) for kv in a.env)
    if not CHROME.is_file():
        sys.exit(f"chrome.exe не найден: {CHROME} (задайте CHROME_EXE)")

    out = Path(a.out) if a.out else REPO / ".tmp" / "site-ab" / time.strftime("%Y%m%d-%H%M%S")
    out.mkdir(parents=True, exist_ok=True)
    probe_js = PROBE_JS.replace("__CH__", json.dumps(a.challenge_selector))
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], cwd=REPO, capture_output=True,
                            text=True).stdout.strip()
    meta = {"url": a.url, "date": time.strftime("%Y-%m-%d %H:%M:%S"), "commit": commit, "args": vars(a) | {
        "exe": str(a.exe), "env": dict(a.env)},
        "route": "напрямую, без прокси (Lumen: LUMEN_NO_SYSTEM_PROXY=1, Chrome: --no-proxy-server)"}
    runs: list[dict] = []
    for k in range(1, a.runs + 1):
        for arm in a.arms.split(","):
            run_dir = out / f"r{k:02d}-{arm}"
            run_dir.mkdir(exist_ok=True)
            t = time.monotonic()
            if arm == "lumen":
                rec = run_lumen(k, a, run_dir, out / "bin", probe_js)
            else:
                rec = asyncio.run(run_chrome(k, a, run_dir, probe_js))
            rec["wall_s"] = round(time.monotonic() - t, 1)
            rec["m"] = metrics(rec, a)
            runs.append(rec)
            (run_dir / "data.json").write_text(json.dumps(rec, ensure_ascii=False), encoding="utf-8")
            m = rec["m"]
            print(f"[{k}] {arm:6s} load {fmt(m['t_load'])} ch_gone {fmt(m['t_ch_gone'])} app_load "
                  f"{fmt(m['t_app_load'])} content {fmt(m['t_content'])} doc_load {fmt(m['doc_load_ms'], 0)}ms "
                  f"lcp {fmt(m['lcp_ms'], 0)}ms res {m['res_n']} cpu {fmt((rec.get('proc') or {}).get('tree_cpu_s'), 1)}s "
                  f"{rec.get('error', '')}", flush=True)
    (out / "results.json").write_text(json.dumps({"meta": meta, "runs": runs}, ensure_ascii=False),
                                      encoding="utf-8")
    print(f"\nresults: {out / 'results.json'}")


if __name__ == "__main__":
    main()
