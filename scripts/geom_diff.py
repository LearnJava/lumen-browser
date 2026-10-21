#!/usr/bin/env python3
"""Differential-срез geometry/computed styles Lumen против Chrome на реальных сайтах (TEST-11).

    python scripts/geom_diff.py [--corpus docs/quality/geom-corpus.txt] [--only slug ...]
        [--lumen target/dev-release/lumen.exe] [--chrome chrome.exe] [--limit 500]
        [--threshold 4] [--settle 2] [--reuse] [--out docs/quality/geom-diff-<дата>.md]

В оба браузера инжектится один и тот же JS-сниппет (`SNAPSHOT_JS`): для каждого отрисованного
элемента `body` (не `display:none` и не внутри такого) — ключ `tag:nth-of-type` от `body`,
`getBoundingClientRect` (округление 0.5px) и срез computed styles (`display`, `position`,
`font-size`, первое имя `font-family`, `visibility`, `overflow`). Подробно описываются до `--limit`
узлов внутри области 2×viewport; ключи и прямоугольники остальных отрисованных узлов идут коротким
списком — по нему отличается «узла нет в DOM» от «узел есть, но вне области / за лимитом».

Lumen ведётся по WebDriver BiDi (`--bidi-port`, токен из stderr, docs/automation.md §tokens),
окно — `--viewport 1280x720` (`browsingContext.setViewport` в bidi-server окно не меняет, только
запоминает размер — `crates/bidi-server/src/protocol.rs`, `bc_set_viewport`). Chrome — headless по
CDP с `Emulation.setDeviceMetricsOverride` 1280×720, DPR 1. Снимок — после
`document.readyState == complete`, инжекта `animation/transition: none` и `--settle` секунд без
изменения числа элементов.

Сырые снимки — `.tmp/geom_diff/<slug>.<браузер>.json` (`--reuse` сравнивает их без браузеров);
отчёт — `docs/quality/geom-diff-<дата>.md` и сводка `geom-diff-<дата>.json` рядом. Как читать —
docs/quality/geom-diff-howto.md.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from datetime import date
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench_chromium import WS, free_port  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
DEFAULT_CORPUS = REPO / "docs" / "quality" / "geom-corpus.txt"
RAW_DIR = REPO / ".tmp" / "geom_diff"
VIEWPORT = (1280, 720)
STYLE_KEYS = ("display", "position", "font-size", "font-family", "visibility", "overflow")

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

# Один и тот же сниппет для обоих браузеров. Синхронный: `script.evaluate` в Lumen не ждёт промисы.
SNAPSHOT_JS = r"""(() => {
  const LIMIT = __LIMIT__, ALL_CAP = 8000;
  const SKIP = {script: 1, style: 1, link: 1, meta: 1, template: 1};
  const vw = window.innerWidth, vh = window.innerHeight, rx = 2 * vw, ry = 2 * vh;
  const h = v => Math.round(v * 2) / 2;
  const nodes = [], all = [], hidden = [];
  let truncated = false;
  const body = document.body;
  if (!body) return JSON.stringify({error: 'no body'});
  const stack = [[body, 'body', []]];
  while (stack.length) {
    const [el, key, ord] = stack.pop();
    const cs = getComputedStyle(el);
    if (cs.display === 'none') { hidden.push(key); continue; }
    const r = el.getBoundingClientRect();
    if (all.length < ALL_CAP) all.push([key, h(r.left), h(r.top), h(r.width), h(r.height)]);
    if (r.right >= 0 && r.bottom >= 0 && r.left < rx && r.top < ry) {
      if (nodes.length < LIMIT) {
        nodes.push({k: key, o: ord, x: h(r.left), y: h(r.top), w: h(r.width), h: h(r.height),
                    s: [cs.display, cs.position, cs.fontSize, cs.fontFamily, cs.visibility, cs.overflow]});
      } else truncated = true;
    }
    const kids = el.children, seen = {}, list = [];
    for (let i = 0; i < kids.length; i++) {
      const t = kids[i].localName;
      if (SKIP[t]) continue;
      seen[t] = (seen[t] || 0) + 1;
      list.push([kids[i], key + '>' + t + ':' + seen[t], ord.concat(i)]);
    }
    for (let i = list.length - 1; i >= 0; i--) stack.push(list[i]);
  }
  const de = document.documentElement;
  return JSON.stringify({url: location.href, title: document.title, vw: vw, vh: vh,
    cw: de.clientWidth, dpr: window.devicePixelRatio, elements: document.getElementsByTagName('*').length,
    truncated: truncated, nodes: nodes, all: all, hidden: hidden});
})()"""

FREEZE_JS = r"""(() => {
  const s = document.createElement('style');
  s.textContent = '*,*::before,*::after{animation:none!important;transition:none!important}';
  (document.head || document.documentElement).appendChild(s);
  window.scrollTo(0, 0);
  return 'ok';
})()"""

PROBE_JS = "document.readyState + '|' + document.getElementsByTagName('*').length"


# ── браузеры ─────────────────────────────────────────────────────────────────

class Rpc:
    """JSON-сообщения поверх WS: `call` ждёт ответ со своим id, события пропускает."""

    def __init__(self, url: str) -> None:
        self.ws = WS(url)
        self.n = 0

    def call(self, method: str, params: dict | None = None, timeout: float = 30) -> dict:
        self.n += 1
        mid = self.n
        self.ws.send(json.dumps({"id": mid, "method": method, "params": params or {}}))
        deadline = time.monotonic() + timeout
        while True:
            left = deadline - time.monotonic()
            if left <= 0:
                raise TimeoutError(method)
            self.ws.sock.settimeout(left)
            try:
                raw = self.ws.recv()
            except socket.timeout:
                raise TimeoutError(method) from None
            try:
                msg = json.loads(raw)
            except ValueError:
                continue  # ping/pong и прочие не-JSON кадры
            if msg.get("id") == mid:
                if "error" in msg:
                    err = msg["error"]
                    raise RuntimeError(f"{method}: {err} {msg.get('message', '')}".strip())
                return msg.get("result", {})

    def close(self) -> None:
        try:
            self.ws.sock.close()
        except OSError:
            pass


def kill_tree(proc: subprocess.Popen) -> None:
    subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"], capture_output=True)


def wait_port(port: int, proc: subprocess.Popen, timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"процесс завершился с кодом {proc.returncode}")
        try:
            socket.create_connection(("127.0.0.1", port), timeout=1).close()
            return
        except OSError:
            time.sleep(0.1)
    raise TimeoutError(f"порт {port} не открылся за {timeout} с")


class Browser:
    name = ""

    def navigate(self, url: str, timeout: float) -> None:
        raise NotImplementedError

    def eval(self, expr: str, timeout: float = 60):
        raise NotImplementedError

    def close(self) -> None:
        raise NotImplementedError

    def snapshot(self, url: str, args) -> dict:
        """Загрузить `url`, дождаться тишины, снять `SNAPSHOT_JS`. Ошибка — `{"error": ...}`."""
        t0 = time.monotonic()
        note = ""
        try:
            self.navigate(url, args.nav_timeout)
        except Exception as e:  # noqa: BLE001 — страница могла частично загрузиться, снимаем всё равно
            note = f"navigate: {e}"
        try:
            deadline = time.monotonic() + args.nav_timeout
            while time.monotonic() < deadline and not str(self.eval(PROBE_JS)).startswith("complete"):
                time.sleep(0.5)
            self.eval(FREEZE_JS)
            last, stable_since, deadline = None, time.monotonic(), time.monotonic() + 20
            while time.monotonic() < deadline:
                cur = self.eval(PROBE_JS)
                if cur != last:
                    last, stable_since = cur, time.monotonic()
                elif time.monotonic() - stable_since >= args.settle:
                    break
                time.sleep(0.25)
            t1 = time.monotonic()
            raw = self.eval(SNAPSHOT_JS.replace("__LIMIT__", str(args.limit)), timeout=120)
            snap = json.loads(raw) if isinstance(raw, str) else {"error": f"снимок вернул {type(raw).__name__}"}
            snap["snapshot_ms"] = round((time.monotonic() - t1) * 1000)
        except Exception as e:  # noqa: BLE001
            snap = {"error": f"{type(e).__name__}: {e}"}
        snap["load_s"] = round(time.monotonic() - t0, 1)
        if note:
            snap["note"] = note
        return snap


class Lumen(Browser):
    name = "lumen"

    def __init__(self, binary: Path) -> None:
        self.binary = binary
        self.proc: subprocess.Popen | None = None
        self.rpc: Rpc | None = None
        self.ctx = ""

    def start(self) -> None:
        port = free_port()
        env = dict(os.environ, LUMEN_NO_ADBLOCK="1")
        self.proc = subprocess.Popen(
            [str(self.binary), "--bidi-port", str(port), "--viewport", f"{VIEWPORT[0]}x{VIEWPORT[1]}"],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env=env,
            encoding="utf-8", errors="replace",
        )
        wait_port(port, self.proc, 60)
        token = None
        for _ in range(400):
            line = self.proc.stderr.readline()
            if not line:
                break
            if line.startswith("[bidi] token: "):
                token = line.strip()[len("[bidi] token: "):]
                break
        if token is None:
            raise RuntimeError("lumen --bidi-port не напечатал [bidi] token")
        threading.Thread(target=lambda: [None for _ in self.proc.stderr], daemon=True).start()
        self.rpc = Rpc(f"ws://127.0.0.1:{port}")
        self.rpc.call("session.new", {"capabilities": {"alwaysMatch": {"token": token}}})
        self.ctx = self.rpc.call("browsingContext.getTree")["contexts"][0]["context"]

    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None

    def navigate(self, url: str, timeout: float) -> None:
        if not self.alive():
            self.close()
            self.start()
        self.rpc.call("browsingContext.navigate", {"context": self.ctx, "url": url, "wait": "complete"}, timeout)

    def eval(self, expr: str, timeout: float = 60):
        deadline = time.monotonic() + timeout
        while True:
            try:
                r = self.rpc.call("script.evaluate", {
                    "expression": expr, "target": {"context": self.ctx}, "awaitPromise": False,
                }, timeout)
                return r.get("result", {}).get("value")
            except RuntimeError as e:
                # JS-рантайм нового документа ставится после readyState (verify_devx6_bidi_scenarios.py).
                if "JS context not available" not in str(e) or time.monotonic() > deadline:
                    raise
                time.sleep(0.3)

    def close(self) -> None:
        if self.rpc:
            self.rpc.close()
            self.rpc = None
        if self.proc:
            kill_tree(self.proc)
            self.proc = None


def find_chrome(explicit: str | None) -> Path:
    """--chrome > $CHROME_BIN > Playwright Chromium (самый новый) > установленный Chrome."""
    if explicit or os.environ.get("CHROME_BIN"):
        return Path(explicit or os.environ["CHROME_BIN"])
    root = Path(os.environ.get("LOCALAPPDATA", "")) / "ms-playwright"
    found = sorted(glob.glob(str(root / "chromium-*" / "chrome-win64" / "chrome.exe")),
                   key=lambda p: int(Path(p).parts[-3].split("-")[1]))
    if found:
        return Path(found[-1])
    for base in (os.environ.get("ProgramFiles", ""), os.environ.get("ProgramFiles(x86)", "")):
        p = Path(base) / "Google" / "Chrome" / "Application" / "chrome.exe"
        if p.is_file():
            return p
    sys.exit("не найден chrome.exe: укажите --chrome")


class Chrome(Browser):
    name = "chrome"

    def __init__(self, binary: Path) -> None:
        self.profile = tempfile.mkdtemp(prefix="geom-chrome-")
        self.port = free_port()
        self.proc = subprocess.Popen([
            str(binary), "--headless=new", f"--remote-debugging-port={self.port}",
            f"--user-data-dir={self.profile}", "--no-first-run", "--no-default-browser-check",
            "--disable-extensions", f"--window-size={VIEWPORT[0]},{VIEWPORT[1]}",
            "--force-device-scale-factor=1", "--lang=en-US", "about:blank",
        ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        deadline = time.monotonic() + 30
        ws_url = None
        while ws_url is None and time.monotonic() < deadline:
            try:
                import urllib.request
                for t in json.load(urllib.request.urlopen(f"http://127.0.0.1:{self.port}/json", timeout=2)):
                    if t.get("type") == "page" and t.get("webSocketDebuggerUrl"):
                        ws_url = t["webSocketDebuggerUrl"]
                        break
            except OSError:
                time.sleep(0.3)
        if ws_url is None:
            self.close()
            raise RuntimeError("Chrome не открыл CDP-цель page")
        self.rpc = Rpc(ws_url)
        self.rpc.call("Page.enable")
        self.rpc.call("Emulation.setDeviceMetricsOverride", {
            "width": VIEWPORT[0], "height": VIEWPORT[1], "deviceScaleFactor": 1, "mobile": False,
        })
        # Полоса прокрутки Lumen не отнимает ширину у страницы (clientWidth = innerWidth), классическая
        # полоса Chrome под Windows отнимает 15px — без выравнивания сдвигается всё, что по центру/справа.
        self.rpc.call("Emulation.setScrollbarsHidden", {"hidden": True})
        ua = self.eval("navigator.userAgent").replace("HeadlessChrome", "Chrome")
        self.rpc.call("Emulation.setUserAgentOverride", {"userAgent": ua, "acceptLanguage": "en-US,en"})

    def navigate(self, url: str, timeout: float) -> None:
        self.rpc.call("Page.navigate", {"url": url}, timeout)

    def eval(self, expr: str, timeout: float = 60):
        r = self.rpc.call("Runtime.evaluate", {"expression": expr, "returnByValue": True}, timeout)
        if "exceptionDetails" in r:
            raise RuntimeError(r["exceptionDetails"].get("text", "exception"))
        return r.get("result", {}).get("value")

    def close(self) -> None:
        if getattr(self, "rpc", None):
            self.rpc.close()
        kill_tree(self.proc)
        for _ in range(10):  # профиль освобождается не сразу
            try:
                shutil.rmtree(self.profile)
                break
            except OSError:
                time.sleep(0.5)


# ── сравнение ────────────────────────────────────────────────────────────────

def norm_style(s: list) -> tuple:
    """Срез стилей к сравнимому виду: `16.0px` = `16px`, первое имя шрифта без кавычек."""
    display, position, size, family, visibility, overflow = (str(v) for v in s)
    try:
        size = f"{round(float(size.removesuffix('px')), 1):g}px"
    except ValueError:
        pass
    family = family.split(",")[0].strip().strip("'\"").lower()
    parts = overflow.split()
    if len(parts) == 2 and parts[0] == parts[1]:
        overflow = parts[0]
    return display, position, size, family, visibility, overflow


def parent_key(key: str) -> str | None:
    return key.rsplit(">", 1)[0] if ">" in key else None


def tag_of(key: str) -> str:
    return key.rsplit(">", 1)[-1].split(":")[0]


def top_tags(tags: list[str], n: int = 3) -> str:
    counts: dict[str, int] = {}
    for t in tags:
        counts[t] = counts.get(t, 0) + 1
    return ", ".join(f"{t}×{c}" for t, c in sorted(counts.items(), key=lambda kv: -kv[1])[:n])


def delta(a: dict, b: dict) -> tuple[float, float, float, float]:
    return b["x"] - a["x"], b["y"] - a["y"], b["w"] - a["w"], b["h"] - a["h"]


def inherited_shift(g: dict, geom: dict, prev_of: dict, parents: set, t: float) -> bool:
    """Расхождение узла объясняется соседом, родителем или детьми — значит, это не корень.

    Сдвиг: такой же, как у родителя, или как у предыдущего соседа (по порядку Chrome) плюс разница
    его высоты/ширины (соседа выше вытянуло). Ширина приходит сверху: своя совпадает или меняется
    как у родителя. Высота приходит снизу: своя совпадает или у узла есть расходящийся ребёнок."""
    dx, dy, dw, dh = g["d"]
    p = parent_key(g["k"])
    pg = geom.get(p) if p else None
    shift_ok = abs(dx) <= t and abs(dy) <= t
    if not shift_ok and pg:
        shift_ok = abs(dx - pg["d"][0]) <= t and abs(dy - pg["d"][1]) <= t
    if not shift_ok and prev_of.get(g["k"]) in geom:
        vx, vy, vw, vh = geom[prev_of[g["k"]]]["d"]
        shift_ok = (abs(dx - vx) <= t and abs(dy - (vy + vh)) <= t) or (abs(dy - vy) <= t and abs(dx - (vx + vw)) <= t)
    width_ok = abs(dw) <= t or (pg is not None and abs(dw - pg["d"][2]) <= t)
    height_ok = abs(dh) <= t or g["k"] in parents
    return shift_ok and width_ok and height_ok


def compare(chrome: dict, lumen: dict, threshold: float) -> dict:
    """Сравнить два снимка. Δ = Lumen − Chrome. Возвращает сводку сайта (см. `site_md`)."""
    cn = {n["k"]: n for n in chrome["nodes"]}
    ln = {n["k"]: n for n in lumen["nodes"]}
    call = {a[0]: a for a in chrome["all"]}
    lall = {a[0]: a for a in lumen["all"]}
    chid, lhid = set(chrome["hidden"]), set(lumen["hidden"])

    geom, style, missing, zero = {}, [], {"lumen": [], "chrome": []}, []
    beyond = 0
    for key in sorted(set(cn) | set(ln)):
        c, lu = cn.get(key), ln.get(key)
        if c and lu:
            d = delta(c, lu)
            if not any((lu["x"], lu["y"], lu["w"], lu["h"])) and any((c["x"], c["y"], c["w"], c["h"])):
                zero.append(key)  # у Lumen нулевой прямоугольник — отдельный класс, геометрию не шумит
            elif max(abs(v) for v in d) > threshold:
                geom[key] = {"k": key, "d": d, "c": c, "l": lu}
            sc, sl = norm_style(c["s"]), norm_style(lu["s"])
            for name, vc, vl in zip(STYLE_KEYS, sc, sl):
                if vc != vl:
                    style.append({"k": key, "prop": name, "c": vc, "l": vl, "tag": tag_of(key)})
            continue
        have, other_name = (c, "lumen") if c else (lu, "chrome")
        other_nodes, other_all, other_hid, other = (
            (lumen["nodes"], lall, lhid, lumen) if c else (chrome["nodes"], call, chid, chrome))
        if key in other_all:
            a = other_all[key]
            if other.get("truncated") and other_nodes and have["o"] > other_nodes[-1]["o"]:
                beyond += 1  # у второго браузера кончился лимит раньше этого узла
                continue
            # Узел есть у обоих, но у одного он вне области 2×viewport — расхождение геометрии.
            o = {"x": a[1], "y": a[2], "w": a[3], "h": a[4]}
            pair = (have, o) if c else (o, have)
            geom[key] = {"k": key, "d": delta(*pair), "c": pair[0], "l": pair[1], "outside": other_name}
            continue
        why = "display:none" if key in other_hid else "нет в DOM"
        if why == "нет в DOM" and any(key.startswith(h + ">") for h in other_hid):
            why = "внутри display:none"
        missing[other_name].append({"k": key, "why": why})

    prev_of, last_child = {}, {}
    for n in chrome["nodes"]:  # порядок документа: предыдущий сосед встречается раньше
        p = parent_key(n["k"])
        if p in last_child:
            prev_of[n["k"]] = last_child[p]
        last_child[p] = n["k"]
    parents = {parent_key(k) for k in geom}
    roots = [g for g in geom.values() if not inherited_shift(g, geom, prev_of, parents, threshold)]
    # Сначала блочный уровень: x/ширина строчных боксов зависят от ширины текста вокруг (метрики
    # шрифта, не раскладка), такие корни идут после блочных при равной величине.
    for g in roots:
        g["inline"] = str(g["c"].get("s", ["inline"])[0]).startswith("inline")
    roots.sort(key=lambda g: (g["inline"], -max(abs(v) for v in g["d"])))

    def subtree_roots(items: list) -> list:
        keys = {i["k"] for i in items}
        out = []
        for i in items:
            p = parent_key(i["k"])
            if p in keys:
                continue
            i["size"] = sum(1 for k in keys if k == i["k"] or k.startswith(i["k"] + ">"))
            out.append(i)
        return sorted(out, key=lambda i: -i["size"])

    by_prop: dict[tuple, list] = {}
    for s in style:
        by_prop.setdefault((s["prop"], s["c"], s["l"]), []).append(s["tag"])
    zero_tags: dict[str, int] = {}
    for key in zero:
        zero_tags[tag_of(key)] = zero_tags.get(tag_of(key), 0) + 1

    compared = len(set(cn) | set(ln)) - beyond
    matched = len(set(cn) & set(ln))
    return {
        "compared": compared,
        "matched": matched,
        "matched_pct": round(100 * matched / compared, 1) if compared else 0.0,
        "beyond_limit": beyond,
        "geom": len(geom),
        "geom_roots": roots,
        "geom_block_roots": sum(1 for g in roots if not g["inline"]),
        "lumen_zero_rect": len(zero),
        "lumen_zero_tags": sorted(zero_tags.items(), key=lambda kv: -kv[1]),
        "style": len(style),
        "style_by_prop": sorted(((k, len(v), top_tags(v)) for k, v in by_prop.items()), key=lambda t: -t[1]),
        "missing_in_lumen": subtree_roots(missing["lumen"]),
        "missing_in_chrome": subtree_roots(missing["chrome"]),
        "missing_total": len(missing["lumen"]) + len(missing["chrome"]),
    }


# ── отчёт ────────────────────────────────────────────────────────────────────

def fmt_d(d) -> str:
    return " ".join(f"{n}{v:+g}" for n, v in zip(("Δx", "Δy", "Δw", "Δh"), d) if v)


def site_md(slug: str, url: str, snaps: dict, cmp: dict | None) -> list[str]:
    out = [f"### {slug} — {url}", ""]
    for name in ("chrome", "lumen"):
        s = snaps[name]
        if "error" in s:
            out.append(f"- {name}: **не снят** — {s['error']}" + (f" ({s['note']})" if s.get("note") else ""))
        else:
            out.append(f"- {name}: {len(s['nodes'])} узлов в области из {s['elements']} элементов, viewport "
                       f"{s['vw']}×{s['vh']} (clientWidth {s['cw']}, DPR {s['dpr']}), загрузка {s['load_s']} с, "
                       f"снимок {s['snapshot_ms']} мс" + (f"; {s['note']}" if s.get("note") else ""))
    if cmp is None:
        return out + [""]
    out.append(f"- сматчено {cmp['matched']} из {cmp['compared']} ({cmp['matched_pct']}%), за лимитом "
               f"{cmp['beyond_limit']}; геометрия: {cmp['geom']} узлов ({len(cmp['geom_roots'])} корней, из них "
               f"блочного уровня {cmp['geom_block_roots']}); "
               f"нулевой прямоугольник у Lumen: {cmp['lumen_zero_rect']}; стиль: {cmp['style']}; "
               f"дерево разошлось: {cmp['missing_total']}")
    if cmp["lumen_zero_tags"]:
        out.append("- нулевой прямоугольник у Lumen по тегам: "
                   + ", ".join(f"{t}×{n}" for t, n in cmp["lumen_zero_tags"][:6]))
    if cmp["geom_roots"]:
        out += ["", "Топ-10 корней расхождения геометрии (Δ = Lumen − Chrome, px):", "",
                "| ключ | Chrome x,y,w,h | Lumen x,y,w,h | Δ |", "|---|---|---|---|"]
        for g in cmp["geom_roots"][:10]:
            c, lu = g["c"], g["l"]
            extra = f" (у {g['outside']} вне области)" if g.get("outside") else ""
            out.append(f"| `{g['k']}` | {c['x']:g},{c['y']:g},{c['w']:g},{c['h']:g} | "
                       f"{lu['x']:g},{lu['y']:g},{lu['w']:g},{lu['h']:g} | {fmt_d(g['d'])}{extra} |")
    if cmp["style_by_prop"]:
        out += ["", "Стиль (свойство: Chrome → Lumen × узлов, теги):", ""]
        out += [f"- `{p}`: {c or '∅'} → {lu or '∅'} ×{n} ({tags})" for (p, c, lu), n, tags in cmp["style_by_prop"][:8]]
    for side, title in (("missing_in_lumen", "нет у Lumen"), ("missing_in_chrome", "нет у Chrome")):
        if cmp[side]:
            out += ["", f"Дерево разошлось — {title} (корни поддеревьев, узлов в поддереве):", ""]
            out += [f"- `{m['k']}` — {m['why']}, {m['size']}" for m in cmp[side][:8]]
    return out + [""]


def clean(cmp: dict) -> bool:
    return cmp["geom"] == 0 and cmp["lumen_zero_rect"] == 0 and cmp["style"] == 0 and cmp["missing_total"] == 0


def summary_line(results: list[dict]) -> str:
    done = [r for r in results if r["cmp"] is not None]
    ok = sum(1 for r in done if clean(r["cmp"]))
    return (f"Итог: {ok} сайтов из {len(done)} без расхождений "
            f"(не сняты: {len(results) - len(done)} из {len(results)})")


def report_md(results: list[dict], meta: dict, threshold: float) -> str:
    out = [
        f"# Geometry/computed-styles diff против Chrome — {meta['date']}", "",
        f"Скрипт `scripts/geom_diff.py` (TEST-11), как читать — [geom-diff-howto.md](geom-diff-howto.md). "
        f"Lumen: `{meta['lumen']}` (сборка {meta['lumen_mtime']}), Chrome: `{meta['chrome']}`. "
        f"Viewport {VIEWPORT[0]}×{VIEWPORT[1]}, порог {threshold:g}px, лимит {meta['limit']} узлов.", "",
        summary_line(results), "",
        "| сайт | узлов C/L | сматчено | геометрия (корней / блочных) | нулевой rect у Lumen | стиль | дерево разошлось |",
        "|---|---|---|---|---|---|---|",
    ]
    for r in results:
        s, c = r["snaps"], r["cmp"]
        if c is None:
            bad = [n for n in ("chrome", "lumen") if "error" in s[n]]
            out.append(f"| {r['slug']} | — | не снят ({', '.join(bad)}) | — | — | — | — |")
            continue
        out.append(f"| {r['slug']} | {len(s['chrome']['nodes'])}/{len(s['lumen']['nodes'])} | "
                   f"{c['matched_pct']}% | {c['geom']} ({len(c['geom_roots'])} / {c['geom_block_roots']}) | {c['lumen_zero_rect']} | {c['style']} | "
                   f"{c['missing_total']} |")
    out += ["", "## По сайтам", ""]
    for r in results:
        out += site_md(r["slug"], r["url"], r["snaps"], r["cmp"])
    return "\n".join(out).rstrip() + "\n"


def summary_json(results: list[dict], meta: dict) -> dict:
    sites = {}
    for r in results:
        c = r["cmp"]
        sites[r["slug"]] = None if c is None else {
            k: c[k] for k in ("compared", "matched", "matched_pct", "beyond_limit", "geom", "lumen_zero_rect", "style",
                                "missing_total")
        } | {"geom_roots": len(c["geom_roots"]), "geom_block_roots": c["geom_block_roots"], "clean": clean(c)}
    return {"meta": meta, "summary": summary_line(results), "sites": sites}


# ── main ─────────────────────────────────────────────────────────────────────

def load_corpus(path: Path, only: list[str]) -> list[tuple[str, str]]:
    sites = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            slug, url = line.split(None, 1)
            if not only or any(o in slug for o in only):
                sites.append((slug, url.strip()))
    if not sites:
        sys.exit(f"корпус пуст: {path} (--only {only})")
    return sites


def default_lumen() -> Path:
    for profile in ("dev-release", "release"):
        p = REPO / "target" / profile / "lumen.exe"
        if p.is_file():
            return p
    return REPO / "target" / "dev-release" / "lumen.exe"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--corpus", default=str(DEFAULT_CORPUS))
    ap.add_argument("--only", nargs="*", default=[])
    ap.add_argument("--lumen", default=str(default_lumen()))
    ap.add_argument("--chrome")
    ap.add_argument("--limit", type=int, default=500)
    ap.add_argument("--threshold", type=float, default=4.0)
    ap.add_argument("--settle", type=float, default=2.0, help="секунд без изменения числа элементов")
    ap.add_argument("--nav-timeout", type=float, default=60.0)
    ap.add_argument("--reuse", action="store_true", help="сравнить сохранённые снимки .tmp/geom_diff без браузеров")
    ap.add_argument("--out", help="отчёт .md (по умолчанию docs/quality/geom-diff-<дата>.md)")
    args = ap.parse_args()

    sites = load_corpus(Path(args.corpus), args.only)
    lumen_bin = Path(args.lumen)
    chrome_bin = find_chrome(args.chrome) if not args.reuse else Path(args.chrome or "-")
    if not args.reuse and not lumen_bin.is_file():
        sys.exit(f"нет бинарника Lumen: {lumen_bin}")
    RAW_DIR.mkdir(parents=True, exist_ok=True)

    browsers: dict[str, Browser] = {}
    if not args.reuse:
        browsers["chrome"] = Chrome(chrome_bin)
        browsers["lumen"] = Lumen(lumen_bin)
        browsers["lumen"].start()
    results = []
    try:
        for slug, url in sites:
            snaps = {}
            for name in ("chrome", "lumen"):
                raw = RAW_DIR / f"{slug}.{name}.json"
                if args.reuse:
                    snaps[name] = json.loads(raw.read_text(encoding="utf-8")) if raw.is_file() else {"error": "нет снимка"}
                    continue
                snaps[name] = browsers[name].snapshot(url, args)
                raw.write_text(json.dumps(snaps[name], ensure_ascii=False), encoding="utf-8")
            ok = all("error" not in s for s in snaps.values())
            cmp = compare(snaps["chrome"], snaps["lumen"], args.threshold) if ok else None
            results.append({"slug": slug, "url": url, "snaps": snaps, "cmp": cmp})
            if cmp:
                print(f"{slug:14} сматчено {cmp['matched_pct']:5}%  геометрия {cmp['geom']:4} "
                      f"({len(cmp['geom_roots'])} корней, блочных {cmp['geom_block_roots']})  стиль {cmp['style']:4}  дерево {cmp['missing_total']}")
            else:
                print(f"{slug:14} не снят: " + "; ".join(f"{n}: {s['error']}" for n, s in snaps.items() if "error" in s))
    finally:
        for b in browsers.values():
            b.close()

    stamp = date.today().isoformat()
    mtime = time.strftime("%Y-%m-%d %H:%M", time.localtime(lumen_bin.stat().st_mtime)) if lumen_bin.is_file() else "?"
    meta = {"date": stamp, "lumen": lumen_bin.relative_to(REPO).as_posix() if lumen_bin.is_relative_to(REPO) else str(lumen_bin),
            "lumen_mtime": mtime, "chrome": chrome_bin.name if args.reuse else str(chrome_bin),
            "limit": args.limit, "threshold": args.threshold}
    out = Path(args.out) if args.out else REPO / "docs" / "quality" / f"geom-diff-{stamp}.md"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(report_md(results, meta, args.threshold), encoding="utf-8", newline="\n")
    out.with_suffix(".json").write_text(
        json.dumps(summary_json(results, meta), ensure_ascii=False, indent=1) + "\n", encoding="utf-8", newline="\n")
    print(summary_line(results))
    print(f"отчёт: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
