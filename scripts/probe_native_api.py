#!/usr/bin/env python3
"""Probe нативности JS-поверхности реалма (FINGERPRINT-JS-REGISTRY, Appendix B).

Один и тот же обход достижимости исполняется внутри браузера в трёх реалмах —
страница, dedicated worker, shared worker — и классифицирует каждую достижимую
функцию по `Function.prototype.toString.call(fn)`: содержит `[native code]` —
нативная, иначе утечка исходника shim-кода. Прогон идёт на пустой локальной
странице (loopback http.server): page-скриптов нет, поэтому «платформенная»
функция — это всё, что обход находит; собственные функции пробы обёрнуты в
IIFE и в глобалы не попадают.

  * `--browser lumen` — `lumen.exe --maximized --mcp-live-port`, eval через
    MCP JSON-RPC (тот же транспорт, что у `.tmp/ws_c_live.py`);
  * `--browser chrome` — Chrome for Testing по CDP (`compat_ab_chrome.py`).

Результат: JSON `{browser, version, realms: {page|dedicated|shared: {...}}}` в
`--out` + сводка в stdout; exit 1, если в каком-либо реалме есть функции с
`native == false` (платформенных page-функций по построению нет).

    python scripts/probe_native_api.py --browser lumen --exe target/dev-release/lumen.exe \
        --out docs/perf/runs/2026-11-19-native-api-lumen.json
    python scripts/probe_native_api.py --browser chrome \
        --out docs/perf/runs/2026-11-19-native-api-chrome.json

Нужен Python ≥ 3.10, только stdlib.
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.request
from datetime import datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bench_chromium import WS, free_port  # noqa: E402
from compat_ab_chrome import find_chrome, lumen_chrome_major  # noqa: E402

REPO = Path(__file__).resolve().parent.parent

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

# ── probe JS ─────────────────────────────────────────────────────────────────
# Самодостаточная функция обхода: root — globalThis/self реалма. Тот же
# алгоритм, что у `_lumen_native_sweep` (data props + акцессоры + прототипная
# цепочка; геттеры вызываются только на «настоящих» объектах, не на
# прототипах), но вместо маскировки — классификация. ES5-синтаксис: код
# исполняется и в page, и в worker-скриптах из blob:.

WALK_FN = r"""
function lumenProbeWalk(root) {
  var out = { functions: [], tags: [], truncated: false };
  var seen = (typeof Set !== 'undefined') ? new Set() : null;
  var seenList = [];
  var budget = 1000000;
  var CAP = 40000;
  var queue = [];
  function mark(o) {
    if (seen) { if (seen.has(o)) return false; seen.add(o); return true; }
    for (var i = 0; i < seenList.length; i++) if (seenList[i] === o) return false;
    seenList.push(o); return true;
  }
  function keyText(k) { return (typeof k === 'symbol') ? ('[' + String(k) + ']') : String(k); }
  function fnNative(f) {
    try { return Function.prototype.toString.call(f).indexOf('[native code]') !== -1; }
    catch (e) { return null; }
  }
  function rec(path, key, f) {
    if (out.functions.length >= CAP) { out.truncated = true; return; }
    var name = '';
    try { name = f.name || ''; } catch (e) {}
    out.functions.push({ path: path, key: keyText(key), name: name, native: fnNative(f) });
  }
  function tag(path, obj) {
    if (out.tags.length >= 20000) return;
    try { out.tags.push({ path: path, tag: Object.prototype.toString.call(obj) }); }
    catch (e) {}
  }
  function harvest(obj, depth, callGetters, path) {
    if (obj === null || (typeof obj !== 'object' && typeof obj !== 'function')) return;
    if (!mark(obj)) return;
    if (budget-- <= 0) { out.truncated = true; return; }
    if (typeof obj !== 'function') tag(path || '(root)', obj);
    var names, syms;
    try { names = Object.getOwnPropertyNames(obj); } catch (e) { names = []; }
    try { syms = Object.getOwnPropertySymbols(obj); } catch (e) { syms = []; }
    for (var pass = 0; pass < 2; pass++) {
      var keys = pass === 0 ? names : syms;
      for (var i = 0; i < keys.length; i++) {
        var key = keys[i], d;
        try { d = Object.getOwnPropertyDescriptor(obj, key); } catch (e) { continue; }
        if (!d) continue;
        var kt = keyText(key), p = path ? (path + '.' + kt) : kt;
        if (typeof d.get === 'function') {
          rec(p, key, d.get);
          if (callGetters && depth < 3) {
            try {
              var gv = d.get.call(obj);
              if (gv !== null && (typeof gv === 'object' || typeof gv === 'function')) {
                queue.push([gv, depth + 1, true, p]);
              }
            } catch (e) {}
          }
        }
        if (typeof d.set === 'function') rec(p, key, d.set);
        var v = d.value;
        if (typeof v === 'function') {
          rec(p, key, v);
          queue.push([v, depth, callGetters, p]);
        } else if (v !== null && typeof v === 'object') {
          queue.push([v, depth, callGetters, p]);
        }
      }
    }
    var proto;
    try { proto = Object.getPrototypeOf(obj); } catch (e) { proto = null; }
    if (proto !== null) queue.push([proto, depth, false, path]);
  }
  queue.push([root, 0, true, '']);
  while (queue.length > 0) {
    var it = queue.pop();
    harvest(it[0], it[1], it[2], it[3]);
  }
  return out;
}
"""

# Код dedicated-воркера: IIFE — свои функции не становятся глобалами реалма.
WORKER_DEDICATED_JS = (
    "(function(){var r;try{r=(" + WALK_FN.strip() + ")(self);}"
    "catch(e){r={error:String(e)};}"
    "postMessage(JSON.stringify(r));"
    "if(typeof self!=='undefined'&&self.close)self.close();})()"
)

# Код shared-воркера: слушатель вешается через addEventListener — функция
# слушателя не own-property globalThis и обходом не собирается.
WORKER_SHARED_JS = (
    "(function(){self.addEventListener('connect',function(e){"
    "var r;try{r=(" + WALK_FN.strip() + ")(self);}catch(x){r={error:String(x)};}"
    "e.ports[0].postMessage(JSON.stringify(r));});})()"
)

# Страничный kickoff: собирает page-результат синхронно, затем поднимает оба
# воркера и ждёт их ответы. Финал — JSON в `__lumen_probe_out`; маркеры
# `__lumen_probe_*` не функции и в обход не попадают (walk уже отработал).
PAGE_KICKOFF_JS = (
    "(function(){"
    "if(typeof globalThis.__lumen_probe_started!=='undefined')return 'busy';"
    "globalThis.__lumen_probe_started=true;"
    "var WALK=" + WALK_FN.strip() + ";"
    "var page;try{page=WALK(globalThis);}catch(e){page={error:String(e)};}"
    "var done=function(){"
    "try{globalThis.__lumen_probe_out=JSON.stringify("
    "{page:page,dedicated:window.__pd||null,shared:window.__ps||null});}catch(e){}"
    "};"
    "window.__pd=null;window.__ps=null;var pend=2;"
    "var fin=function(){if(--pend===0)done();};"
    "try{"
    "var w=new Worker(URL.createObjectURL(new Blob([" + json.dumps(WORKER_DEDICATED_JS) + "],{type:'text/javascript'})));"
    "w.onmessage=function(ev){window.__pd=JSON.parse(ev.data);fin();};"
    "w.onerror=function(){window.__pd={error:'worker error'};fin();};"
    "}catch(e){window.__pd={error:'dedicated: '+String(e)};fin();}"
    "try{"
    "var sw=new SharedWorker(URL.createObjectURL(new Blob([" + json.dumps(WORKER_SHARED_JS) + "],{type:'text/javascript'})));"
    "sw.port.onmessage=function(ev){window.__ps=JSON.parse(ev.data);fin();};"
    "sw.port.onerror=function(){window.__ps={error:'sharedworker error'};fin();};"
    "sw.port.start();"
    "}catch(e){window.__ps={error:'shared: '+String(e)};fin();}"
    "return 'started';})()"
)

POLL_JS = "typeof globalThis.__lumen_probe_out==='string'?globalThis.__lumen_probe_out:'__pending__'"

TIMEOUT_S = 90.0


# ── локальный HTTP-сервер для пробы (loopback = secure context → blob-воркеры) ──

class _ProbeHandler(BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        body = b"<!doctype html><title>native-api probe</title>"
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):  # тихий сервер
        pass


def probe_url() -> tuple[str, ThreadingHTTPServer]:
    srv = ThreadingHTTPServer(("127.0.0.1", 0), _ProbeHandler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    return f"http://127.0.0.1:{srv.server_address[1]}/", srv


# ── Lumen: MCP live session ──────────────────────────────────────────────────

class LumenSession:
    """`lumen.exe --mcp-live-port`: JSON-RPC по TCP, eval через tools/call."""

    def __init__(self, exe: Path, url: str, env_extra: dict[str, str]) -> None:
        self.port = free_port()
        env = dict(os.environ)
        env.update(env_extra)
        self.err_path = REPO / ".tmp" / f"probe-native-{os.getpid()}.log"
        self.err_path.parent.mkdir(parents=True, exist_ok=True)
        self._err = open(self.err_path, "w", encoding="utf-8", errors="replace")
        self.proc = subprocess.Popen(
            [str(exe), "--maximized", "--mcp-live-port", str(self.port), url],
            stdout=subprocess.DEVNULL, stderr=self._err, env=env,
        )
        self._token = self._read_token(45)
        self._sock = socket.create_connection(("127.0.0.1", self.port), timeout=15)
        self._sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        self._sock.settimeout(120)
        self._rd = self._sock.makefile("r", encoding="utf-8", newline="\n")
        self._id = 0
        self._call("initialize", {"token": self._token})
        try:
            self._call("tools/call", {"name": "wait", "arguments": {"condition": "document_ready", "timeout_ms": 30000}})
        except Exception:
            pass  # probe-страница пустая; ждать нечего

    def _read_token(self, timeout: float) -> str:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                for line in open(self.err_path, encoding="utf-8", errors="replace"):
                    if line.startswith("[mcp] token: "):
                        return line.strip()[len("[mcp] token: "):]
            except OSError:
                pass
            if self.proc.poll() is not None:
                raise RuntimeError("lumen exited before MCP token")
            time.sleep(0.2)
        raise RuntimeError("no [mcp] token in stderr")

    def _call(self, method: str, params: dict) -> dict:
        self._id += 1
        self._sock.sendall(
            (json.dumps({"jsonrpc": "2.0", "id": self._id, "method": method, "params": params}) + "\n").encode()
        )
        line = self._rd.readline()
        if not line:
            raise RuntimeError("MCP socket closed")
        r = json.loads(line)
        if r.get("error"):
            raise RuntimeError(str(r["error"]))
        res = r.get("result") or {}
        if res.get("isError"):
            raise RuntimeError(str(res.get("content")))
        return res

    def eval(self, code: str, timeout_ms: int = 110000):
        res = self._call("tools/call", {"name": "eval", "arguments": {"code": code, "timeout_ms": timeout_ms}})
        content = res.get("content") if isinstance(res, dict) else None
        if content:
            try:
                payload = json.loads(content[0].get("text", "{}"))
            except Exception:
                return content
            # MCP eval заворачивает значение: {"result": <JSON.stringify(js)>}
            if isinstance(payload, dict) and "result" in payload:
                inner = payload["result"]
                if isinstance(inner, str):
                    try:
                        return json.loads(inner)
                    except Exception:
                        return inner
                return inner
            return payload
        return res

    def close(self) -> None:
        try:
            self.proc.kill()
        except Exception:
            pass
        try:
            self._sock.close()
            self._err.close()
        except Exception:
            pass


# ── Chrome: CDP ──────────────────────────────────────────────────────────────

def page_ws_url(port: int, timeout: float = 30) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            data = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json", timeout=2))
            for t in data:
                if t.get("type") == "page" and t.get("webSocketDebuggerUrl"):
                    return t["webSocketDebuggerUrl"]
        except OSError:
            pass
        time.sleep(0.3)
    raise RuntimeError("нет CDP-цели page")


class ChromeSession:
    def __init__(self, binary: Path, url: str) -> None:
        self.port = free_port()
        profile = tempfile.mkdtemp(prefix="probe-native-chrome-")
        self.proc = subprocess.Popen(
            [
                str(binary), f"--remote-debugging-port={self.port}",
                f"--user-data-dir={profile}", "--headless=new",
                "--no-first-run", "--no-default-browser-check", "--disable-extensions",
                "--window-size=800,600", url,
            ],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        self.page = _CdpPage(WS(page_ws_url(self.port)))
        self.page.call("Page.enable")
        self.page.call("Runtime.enable")
        # Явная навигация: URL-аргумент командной строки в headless может
        # оставить цель на about:blank (opaque origin → blob-воркеры denied).
        self.page.call("Page.navigate", {"url": url})
        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            try:
                if self.page.eval("location.href") == url:
                    break
            except Exception:
                pass
            time.sleep(0.3)

    def eval(self, code: str):
        return self.page.eval(code, timeout=110)

    def close(self) -> None:
        try:
            self.proc.kill()
        except Exception:
            pass


class _CdpPage:
    """Минимальный CDP-клиент: send/call/eval без сбора событий."""

    def __init__(self, ws: WS) -> None:
        self.ws = ws
        self.n = 0

    def send(self, method: str, params: dict | None = None) -> int:
        self.n += 1
        self.ws.send(json.dumps({"id": self.n, "method": method, "params": params or {}}))
        return self.n

    def call(self, method: str, params: dict | None = None, timeout: float = 30) -> dict:
        mid = self.send(method, params)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self.ws.sock.settimeout(max(0.1, deadline - time.monotonic()))
            try:
                msg = json.loads(self.ws.recv())
            except (socket.timeout, TimeoutError):
                break
            except OSError:
                break
            if msg.get("id") == mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})
        raise TimeoutError(method)

    def eval(self, expr: str, timeout: float = 30):
        r = self.call(
            "Runtime.evaluate",
            {"expression": expr, "returnByValue": True, "awaitPromise": True},
            timeout,
        )
        return r.get("result", {}).get("value")


# ── драйвер прогона ──────────────────────────────────────────────────────────

def run_probe(session, label: str) -> dict:
    """Запустить kickoff и дождаться `__lumen_probe_out`."""
    session.eval(PAGE_KICKOFF_JS)
    deadline = time.monotonic() + TIMEOUT_S
    last = None
    while time.monotonic() < deadline:
        try:
            v = session.eval(POLL_JS)
        except Exception as ex:
            v = f"__eval_error__ {ex}"
        if isinstance(v, str) and v != "__pending__":
            try:
                data = json.loads(v)
                # MCP eval может отдать JSON-строку внутри JSON-строки.
                if isinstance(data, str):
                    data = json.loads(data)
            except Exception:
                data = {"raw": str(v)[:2000]}
            data["_browser"] = label
            return data
        last = v
        time.sleep(1.0)
    return {"error": f"probe timeout ({TIMEOUT_S}s); last poll: {last!r}"}


def summarize(realm_name: str, realm: dict | None) -> dict:
    if not isinstance(realm, dict):
        return {"realm": realm_name, "error": "missing"}
    if "error" in realm and "functions" not in realm:
        return {"realm": realm_name, "error": realm["error"]}
    funcs = realm.get("functions") or []
    leaks = [f for f in funcs if f.get("native") is False]
    unknown = [f for f in funcs if f.get("native") is None]
    return {
        "realm": realm_name,
        "functions": len(funcs),
        "leaks": len(leaks),
        "unclassified": len(unknown),
        "tags": len(realm.get("tags") or []),
        "truncated": bool(realm.get("truncated")),
        "leak_paths": [f["path"] for f in leaks[:50]],
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--browser", choices=["lumen", "chrome"], required=True)
    ap.add_argument("--exe", help="lumen.exe (для --browser lumen)")
    ap.add_argument("--chrome-binary", help="chrome.exe (для --browser chrome; иначе CfT-кэш)")
    ap.add_argument("--out", required=True, help="куда писать JSON-леджер")
    ap.add_argument("--env", action="append", default=[], help="K=V окружения для lumen")
    args = ap.parse_args()

    url, srv = probe_url()
    env_extra = dict(kv.split("=", 1) for kv in args.env)
    session = None
    try:
        if args.browser == "lumen":
            exe = Path(args.exe or "target/dev-release/lumen.exe")
            if not exe.is_file():
                sys.exit(f"нет бинарника: {exe}")
            session = LumenSession(exe, url, env_extra)
            version = "lumen"
        else:
            binary, cft_ver = find_chrome(args.chrome_binary, lumen_chrome_major())
            session = ChromeSession(binary, url)
            version = cft_ver

        raw = run_probe(session, args.browser)
        out = {
            "browser": args.browser,
            "version": version,
            "date": datetime.now().isoformat(timespec="seconds"),
            "url": url,
        }
        # Полный ledger: по реализмам — functions/tags как их отдал обход.
        realms = raw if isinstance(raw, dict) and "page" in raw else {"page": raw}
        realms.pop("_browser", None)
        for name, realm in list(realms.items()):
            if realm is not None and not isinstance(realm, dict):
                realms[name] = {"_raw": str(realm)[:500], "_type": type(realm).__name__}
        out["realms"] = {
            name: {k: v for k, v in (realm or {}).items() if k != "_browser"}
            for name, realm in realms.items()
        }
        summary = {name: summarize(name, realm) for name, realm in realms.items()}
        out["summary"] = summary

        out_path = Path(args.out)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_text(json.dumps(out, ensure_ascii=False, indent=1), encoding="utf-8")

        print(f"probe: {args.browser} {version} → {out_path}")
        bad = False
        for name, s in summary.items():
            if "error" in s:
                print(f"  {name}: ERROR {s['error']}")
                bad = True
            else:
                print(
                    f"  {name}: functions={s['functions']} leaks={s['leaks']} "
                    f"unclassified={s['unclassified']} tags={s['tags']}"
                )
                if s["leaks"]:
                    bad = True
                    for p in s["leak_paths"][:20]:
                        print(f"    leak: {p}")
        return 1 if bad else 0
    finally:
        if session is not None:
            session.close()
        srv.shutdown()


if __name__ == "__main__":
    sys.exit(main())
