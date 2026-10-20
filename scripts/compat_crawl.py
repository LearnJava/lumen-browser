#!/usr/bin/env python3
"""Краулер паритета Lumen с эталоном Chrome (COMPAT-CORPUS-1, дорожка COMPAT).

Отдельный инструмент, не режим `perf_audit.py`: там `--mode compat` — процесс на
сайт ради сопоставимых перф-чисел, здесь — сравнение с эталоном (BUG-992 — не
смешивать). Общее берётся у `perf_audit.py` (живое окно `LiveBrowser`, MCP,
`classify_status`, `count_sigs`, разбор сетевого лога) и у
`chrome_interaction_baseline.py` (CDP-клиент, поиск Chrome for Testing).

На каждый сайт — два плеча на одном вьюпорте (размер берётся у окна Lumen):
Lumen (живое окно, MCP) и Chrome (CDP, свой изолированный контекст на сайт).
Сайт в паритете, если прошёл все 5 проверок
(docs/research/site-compat/strategies/engineering-process.md §3.3):

  1 status      итоговый HTTP-статус того же класса (2xx/3xx/4xx/5xx), что у Chrome;
  2 walls       нет стены (проверка браузера, WAF, «не поддерживается», пустой белый
                экран), которой нет у Chrome; классификатор — scripts/wall_signatures.py;
  3 exceptions  нет новых необработанных исключений / console.error (по нормализованному
                тексту): сигнатуры Lumen минус сигнатуры Chrome;
  4 pixels      доля отличающихся блоков кадра ниже порога после маскировки динамики
                (картинки, видео, iframe, баннеры, карусели, анимации, даты/время);
  5 a11y        ≥95% интерактивных узлов Chrome (роль + имя) есть в дереве Lumen
                (resource://a11y_tree против Accessibility.getFullAXTree).

Расхождение — повторный прогон обоих плеч: проверка считается проваленной, только
если провалилась в обоих прогонах; проверка с разным исходом — флап. Итог:
CPR — доля сайтов в паритете среди сайтов, где посчитаны все 5 проверок.

Результат: .tmp/compat-crawl/<timestamp>/ (results.jsonl — по строке на сайт,
summary.md, кадры, логи) — НЕ коммитится. `--resume DIR` продолжает прерванный прогон.

Примеры:
  python scripts/compat_crawl.py --only example,lenta
  python scripts/compat_crawl.py --corpus docs/perf/corpus.txt --corpus docs/perf/corpus-top100-foreign.txt
  python scripts/compat_crawl.py --resume .tmp/compat-crawl/20261013-120000
"""

from __future__ import annotations

import argparse
import base64
import importlib.util
import json
import math
import os
import re
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from collections import Counter
from datetime import datetime
from pathlib import Path

SCRIPTS_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPTS_DIR.parent
sys.path.insert(0, str(SCRIPTS_DIR))

import captcha_stand as cs  # noqa: E402  (NET_ERROR_RE — страница сетевой ошибки самого браузера)
import chrome_interaction_baseline as cib  # noqa: E402
import perf_audit as pa  # noqa: E402
import wall_signatures as ws  # noqa: E402
from bench_chromium import WS, free_port  # noqa: E402

DEFAULT_CORPORA = [
    REPO_ROOT / "docs" / "perf" / "corpus.txt",
    REPO_ROOT / "docs" / "perf" / "corpus-top100-foreign.txt",
]
OUT_ROOT = REPO_ROOT / ".tmp" / "compat-crawl"

CHECKS = ("status", "walls", "exceptions", "pixels", "a11y")
# Роли a11y, по которым считается «набор интерактивных элементов» (проверка 5).
INTERACTIVE_ROLES = frozenset({
    "button", "link", "textbox", "searchbox", "combobox", "checkbox", "radio", "switch",
    "slider", "spinbutton", "menuitem", "menuitemcheckbox", "menuitemradio", "tab", "option",
})
BLOCK_PX = 16  # сторона блока сравнения кадров
TEXT_LIMIT = 30000
NEAR_EMPTY_LUMEN = 200   # знаков видимого текста у Lumen
NEAR_EMPTY_CHROME = 1000  # …при столько же и более у Chrome → «белый экран»

# ── Общий JS-зонд (исполняется в обоих плечах одинаково) ─────────────────────

PAGE_JS = r"""(() => {
  const o = {};
  const rects = [];
  const add = el => {
    try {
      const r = el.getBoundingClientRect();
      if (r.width > 1 && r.height > 1 && r.bottom > 0 && r.top < innerHeight && r.right > 0 && r.left < innerWidth)
        rects.push([Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)]);
    } catch (e) {}
  };
  try { o.text = ((document.body && (document.body.innerText || document.body.textContent)) || '').slice(0, %(limit)d); } catch (e) { o.text = ''; }
  try { o.title = document.title.slice(0, 160); } catch (e) { o.title = ''; }
  try { o.href = location.href.slice(0, 300); } catch (e) { o.href = ''; }
  try { o.viewport = [innerWidth, innerHeight, devicePixelRatio]; } catch (e) { o.viewport = null; }
  try {
    document.querySelectorAll('img,picture,video,canvas,iframe,embed,object,ins,marquee,[aria-live],' +
      '[class*="banner"],[class*="advert"],[id*="advert"],[class*="carousel"],[class*="slider"],' +
      '[class*="swiper"],[class*="adv-"],[class*="ad-"],[id^="ad-"],[id^="ad_"]').forEach(add);
  } catch (e) {}
  try {
    const dateRe = /(\d{1,2}[:.]\d{2})|(\d{1,2}[./]\d{1,2}[./]\d{2,4})|(\d{4}-\d{2}-\d{2})|(\d{1,2}\s+(янв|фев|мар|апр|ма[йя]|июн|июл|авг|сен|окт|ноя|дек|jan|feb|mar|apr|may|jun|jul|aug|sep|oct|nov|dec))|(\d+\s+(мин|час|ден|дн|min|hour|day|sec|сек)\w*\s+(назад|ago))|(сегодня|вчера|today|yesterday)/i;
    document.querySelectorAll('time,span,a,div,p,li,small,b,i').forEach(el => {
      if (rects.length > 3000 || el.children.length) return;
      const t = (el.textContent || '').trim();
      if (t.length > 0 && t.length < 40 && dateRe.test(t)) add(el);
    });
  } catch (e) {}
  try {
    if (document.getAnimations) document.getAnimations().forEach(a => { const t = a.effect && a.effect.target; if (t) add(t); });
  } catch (e) {}
  o.masks = rects.slice(0, 3000);
  return JSON.stringify(o);
})()""" % {"limit": TEXT_LIMIT}

# ── Проверки (чистые функции, без ввода-вывода) ──────────────────────────────


def status_class(code: int | None) -> str:
    return "none" if not code else f"{code // 100}xx"


def check_status(chrome: dict, lumen: dict) -> dict:
    c, l = status_class(chrome.get("http_status")), status_class(lumen.get("http_status"))
    return {"ok": c == l, "detail": f"chrome={chrome.get('http_status')} lumen={lumen.get('http_status')}"}


def wall_hits(snap: dict) -> set[str]:
    """Стены на снимке плеча: `жёсткая|мягкая:id сигнала` по общему классификатору.

    Тексту доверяем и как телу, и как видимому тексту: сырого HTML у снимка нет,
    а подписи вендоров (`Just a moment...`, `Attention Required`) видны на странице.
    """
    text = snap.get("text") or ""
    r = ws.classify_wall(snap.get("http_status"), None, body=text, visible_text=text)
    if r["kind"] not in ("hard", "soft"):
        return set()
    return {f"{r['kind']}:{sid}" for sid in r["signals"]}


def is_net_error_page(snap: dict) -> bool:
    """Снимок — страница сетевой ошибки самого браузера (у Lumen — net_error_page.rs)."""
    return bool(cs.NET_ERROR_RE.search((snap.get("title") or "") + "\n" + (snap.get("text") or "")[:400]))


def check_walls(chrome: dict, lumen: dict) -> dict:
    ch, lu = wall_hits(chrome), wall_hits(lumen)
    lumen_blank = len((lumen.get("text") or "").strip()) < NEAR_EMPTY_LUMEN
    if lumen_blank and len((chrome.get("text") or "").strip()) >= NEAR_EMPTY_CHROME:
        lu.add("blank:near-empty")
    if is_net_error_page(lumen) and not is_net_error_page(chrome):
        # Страница ошибки Lumen — 650 знаков текста, «пустым экраном» не считается; без этой стены
        # первый прогон проходил проверку, а повтор (таймаут `wait`, текста нет) проваливал — ложный флап.
        lu.add("net-error")
    extra = sorted(lu - ch)
    return {"ok": not extra, "detail": ", ".join(extra) if extra else "", "extra": extra,
            "chrome_hits": sorted(ch), "lumen_hits": sorted(lu)}


_PREFIX_RE = re.compile(r"^(?:\[[^\]]*\]\s*)*(?:uncaught\s+)?(?:\(in promise\)\s+)?", re.IGNORECASE)
_URL_RE = re.compile(r"https?://\S+|\bblob:\S+")


def exc_sig(message: str) -> str:
    """Нормализованная сигнатура исключения: без `Uncaught`, URL, цифр и хешей."""
    first = (message or "").strip().split("\n", 1)[0]
    first = _PREFIX_RE.sub("", first)
    first = _URL_RE.sub("URL", first)
    return pa.error_sig(first)[:160]


def check_exceptions(chrome: dict, lumen: dict) -> dict:
    ch = {exc_sig(m) for m in chrome.get("exceptions") or []}
    lu = Counter(exc_sig(m) for m in lumen.get("exceptions") or [])
    new = sorted(s for s in lu if s and s not in ch)
    return {"ok": not new, "detail": "; ".join(new[:3]), "new_sigs": new,
            "chrome_n": len(ch), "lumen_n": sum(lu.values())}


def norm_name(name: str) -> str:
    return " ".join((name or "").lower().split())[:80]


def flatten_lumen_ax(tree: dict | None) -> list[tuple[str, str]]:
    """Интерактивные (роль, имя) из resource://a11y_tree; скрытые поддеревья пропускаются."""
    out, stack = [], [tree] if tree else []
    while stack:
        n = stack.pop()
        if (n.get("state") or {}).get("hidden"):
            continue
        if n.get("role") in INTERACTIVE_ROLES:
            out.append((n["role"], norm_name(n.get("name", ""))))
        stack.extend(n.get("children") or [])
    return out


def flatten_chrome_ax(nodes: list[dict]) -> list[tuple[str, str]]:
    out = []
    for n in nodes:
        if n.get("ignored"):
            continue
        role = (n.get("role") or {}).get("value")
        if role in INTERACTIVE_ROLES:
            out.append((role, norm_name((n.get("name") or {}).get("value", ""))))
    return out


def ax_coverage(chrome_ax: list, lumen_ax: list) -> dict:
    """Доля интерактивных узлов Chrome, нашедших пару (роль, имя) в дереве Lumen."""
    cc, lc = Counter(map(tuple, chrome_ax)), Counter(map(tuple, lumen_ax))
    matched = sum(min(n, lc[k]) for k, n in cc.items())
    n_chrome = sum(cc.values())
    missing = sorted(((cc[k] - lc[k], k) for k in cc if cc[k] > lc[k]), reverse=True)[:8]
    return {"n_chrome": n_chrome, "n_lumen": sum(lc.values()), "matched": matched,
            "coverage": None if n_chrome == 0 else round(matched / n_chrome, 4),
            "missing": [[r, nm, c] for c, (r, nm) in missing]}


def check_a11y(chrome_ax: list | None, lumen_ax: list | None, threshold: float) -> dict:
    if chrome_ax is None or lumen_ax is None:
        return {"ok": False, "detail": "нет a11y-дерева", "coverage": None}
    cov = ax_coverage(chrome_ax, lumen_ax)
    ok = cov["coverage"] is None or cov["coverage"] >= threshold
    cov.update(ok=ok, detail=f"{cov['matched']}/{cov['n_chrome']} (Lumen {cov['n_lumen']})")
    return cov


def block_means(w: int, h: int, bpp: int, px: bytes, block: int = BLOCK_PX) -> tuple[int, int, list]:
    """Средний цвет блоков block×block: (nbx, nby, [(r,g,b)…]); неполный край отбрасывается."""
    nbx, nby = w // block, h // block
    stride, chans = w * bpp, (3 if bpp >= 3 else 1)
    out = []
    for by in range(nby):
        acc = [[0, 0, 0] for _ in range(nbx)]
        for y in range(by * block, (by + 1) * block):
            row = px[y * stride:(y + 1) * stride]
            for c in range(chans):
                chan = row[c::bpp]
                for bx in range(nbx):
                    acc[bx][c] += sum(chan[bx * block:(bx + 1) * block])
        div = block * block
        for a in acc:
            r = a[0] // div
            out.append((r, r, r) if chans == 1 else (r, a[1] // div, a[2] // div))
    return nbx, nby, out


def mask_blocks(masks: list, scale: float, nbx: int, nby: int, block: int = BLOCK_PX) -> set[tuple[int, int]]:
    """Блоки сетки, задетые прямоугольниками масок (CSS-пиксели × scale -> пиксели кадра)."""
    hit = set()
    for x, y, w, h in masks:
        x0, y0 = max(0, int(x * scale) // block), max(0, int(y * scale) // block)
        # правая/нижняя граница эксклюзивна: ceil(...) - 1 — последний задетый пиксель
        x1 = min(nbx - 1, (math.ceil((x + w) * scale) - 1) // block)
        y1 = min(nby - 1, (math.ceil((y + h) * scale) - 1) // block)
        for by in range(y0, y1 + 1):
            for bx in range(x0, x1 + 1):
                hit.add((bx, by))
    return hit


def frame_diff(a: tuple, b: tuple, masks: list, scale: float, tol: int) -> dict:
    """Доля отличающихся блоков кадров a и b (результаты `block_means`) вне масок."""
    nbx, nby = min(a[0], b[0]), min(a[1], b[1])
    masked = mask_blocks(masks, scale, nbx, nby)
    compared = differing = 0
    for by in range(nby):
        for bx in range(nbx):
            if (bx, by) in masked:
                continue
            pa_, pb_ = a[2][by * a[0] + bx], b[2][by * b[0] + bx]
            compared += 1
            if max(abs(pa_[0] - pb_[0]), abs(pa_[1] - pb_[1]), abs(pa_[2] - pb_[2])) > tol:
                differing += 1
    total = nbx * nby
    return {"blocks": total, "masked": len(masked), "compared": compared, "differing": differing,
            "diff_pct": round(100.0 * differing / compared, 2) if compared else None,
            "mask_pct": round(100.0 * len(masked) / total, 1) if total else None}


def check_pixels(chrome_frame: tuple | None, lumen_frame: tuple | None, masks: list, scale: float,
                 threshold: float, tol: int) -> dict:
    if chrome_frame is None or lumen_frame is None:
        return {"ok": False, "detail": "нет кадра", "diff_pct": None}
    d = frame_diff(chrome_frame, lumen_frame, masks, scale, tol)
    d["ok"] = d["diff_pct"] is not None and d["diff_pct"] <= threshold
    d["detail"] = f"diff {d['diff_pct']}% (порог {threshold}%), маска {d['mask_pct']}%"
    return d


def evaluate(chrome: dict, lumen: dict, args) -> dict:
    """Пять проверок по снимкам двух плеч -> {имя: {ok, detail, …}}."""
    scale = (chrome.get("scale") or lumen.get("scale") or 1.0)
    return {
        "status": check_status(chrome, lumen),
        "walls": check_walls(chrome, lumen),
        "exceptions": check_exceptions(chrome, lumen),
        "pixels": check_pixels(chrome.get("frame"), lumen.get("frame"),
                               (chrome.get("masks") or []) + (lumen.get("masks") or []),
                               scale, args.px_threshold, args.px_tolerance),
        "a11y": check_a11y(chrome.get("ax"), lumen.get("ax"), args.ax_threshold),
    }


def combine_runs(first: dict, second: dict | None) -> tuple[dict, list[str]]:
    """Итог по двум прогонам: провал — только если в обоих; (проверки, флапы)."""
    if second is None:
        return {k: dict(v) for k, v in first.items()}, []
    final, flaps = {}, []
    for k in CHECKS:
        a, b = first[k], second[k]
        final[k] = dict(b if not a["ok"] and not b["ok"] else (a if a["ok"] else b))
        final[k]["ok"] = a["ok"] or b["ok"]
        if a["ok"] != b["ok"]:
            flaps.append(k)
    return final, flaps


def summarize(records: list[dict]) -> dict:
    """CPR, доля посчитанных проверок, флапы, провалы по проверкам."""
    n = len(records)
    computed = [r for r in records if r.get("computed")]
    parity = [r for r in computed if all(r["checks"][k]["ok"] for k in CHECKS)]
    per_check = {k: sum(1 for r in computed if r["checks"][k]["ok"]) for k in CHECKS}
    return {
        "sites": n, "computed": len(computed),
        "computed_pct": round(100.0 * len(computed) / n, 1) if n else None,
        "ref_failed": sum(1 for r in records if r.get("ref_failed")),
        "parity": len(parity),
        "cpr_pct": round(100.0 * len(parity) / len(computed), 1) if computed else None,
        "per_check_pass": per_check,
        "reran": sum(1 for r in computed if r.get("reran")),
        "flapped": sum(1 for r in computed if r.get("flaps")),
        "flap_pct": round(100.0 * sum(1 for r in computed if r.get("flaps")) / len(computed), 1) if computed else None,
    }


# ── Плечо Chrome ─────────────────────────────────────────────────────────────


def pump(cdp, seconds: float) -> None:
    """Читать события CDP `seconds` секунд (cib.Cdp копит их в cdp.events)."""
    end = time.monotonic() + seconds
    while True:
        left = end - time.monotonic()
        if left <= 0:
            break
        cdp.ws.sock.settimeout(min(left, 1.0))
        try:
            msg = json.loads(cdp.ws.recv())
        except (socket.timeout, TimeoutError):
            continue
        if "method" in msg:
            cdp.events.append(msg)
    cdp.ws.sock.settimeout(30)


def parse_probe(raw) -> dict:
    """Ответ PAGE_JS (строка JSON, у Lumen eval иногда кодирует дважды) -> dict."""
    for _ in range(3):
        if isinstance(raw, str):
            try:
                raw = json.loads(raw)
            except json.JSONDecodeError:
                break
    return raw if isinstance(raw, dict) else {}


def chrome_exception_text(params: dict) -> str:
    d = params.get("exceptionDetails") or {}
    exc = d.get("exception") or {}
    return (exc.get("description") or exc.get("value") or d.get("text") or "").strip()


class ChromeRef:
    """Один процесс Chrome for Testing на прогон; на сайт — свой изолированный контекст."""

    def __init__(self, binary: str, headless: bool, work: Path) -> None:
        self.binary, self.headless, self.work = binary, headless, work
        self.proc = None
        self.port = 0
        self.browser = None
        self._spawn()

    def _spawn(self) -> None:
        profile = self.work / "chrome-profile"
        shutil.rmtree(profile, ignore_errors=True)
        last: Exception | None = None
        for _ in range(2):
            self.port = free_port()
            cmd = [self.binary, f"--remote-debugging-port={self.port}", f"--user-data-dir={profile}",
                   "--no-first-run", "--no-default-browser-check", "--disable-backgrounding-occluded-windows",
                   "--disable-renderer-backgrounding", "--lang=ru-RU", "about:blank"]
            cmd.insert(1, "--headless=new" if self.headless else "--start-maximized")
            self.proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                deadline = time.monotonic() + 45
                while time.monotonic() < deadline:
                    try:
                        info = json.load(urllib.request.urlopen(f"http://127.0.0.1:{self.port}/json/version", timeout=2))
                        self.browser = cib.Cdp(WS(info["webSocketDebuggerUrl"]))
                        return
                    except OSError:
                        time.sleep(0.3)
                raise RuntimeError("CDP Chrome не поднялся")
            except RuntimeError as e:
                last = e
                self.proc.kill()
                time.sleep(2)
        raise RuntimeError(str(last))

    def alive(self) -> bool:
        return self.proc is not None and self.proc.poll() is None

    def restart(self) -> None:
        self.close()
        self._spawn()

    def close(self) -> None:
        if self.proc is not None:
            self.proc.terminate()
            try:
                self.proc.wait(timeout=8)
            except subprocess.TimeoutExpired:
                self.proc.kill()

    def snapshot(self, url: str, viewport: list[float], args, out_png: Path) -> dict:
        snap: dict = {"arm": "chrome", "ok": False}
        ctx = tid = cdp = None
        try:
            ctx = self.browser.call("Target.createBrowserContext")["browserContextId"]
            tid = self.browser.call("Target.createTarget", {"url": "about:blank", "browserContextId": ctx})["targetId"]
            cdp = cib.Cdp(WS(f"ws://127.0.0.1:{self.port}/devtools/page/{tid}"))
            for dom in ("Page", "Network", "Runtime"):
                cdp.call(f"{dom}.enable")
            iw, ih, dpr = viewport
            cdp.call("Emulation.setDeviceMetricsOverride", {"width": int(iw), "height": int(ih),
                                                            "deviceScaleFactor": dpr, "mobile": False})
            if self.headless:
                ua = cdp.eval("navigator.userAgent") or ""
                cdp.call("Network.setUserAgentOverride", {"userAgent": ua.replace("HeadlessChrome", "Chrome")})
            t0 = time.monotonic()
            nav = cdp.call("Page.navigate", {"url": url}, timeout=args.timeout)
            if nav.get("errorText"):
                snap["net_error"] = nav["errorText"]
            main_frame = nav.get("frameId")
            ready_expr = "location.href !== 'about:blank' && document.readyState === 'complete'"
            while time.monotonic() - t0 < args.timeout and not snap.get("net_error"):
                if cdp.eval(ready_expr) is True:
                    snap["ready"] = True
                    break
                pump(cdp, 0.5)
            snap["ready_s"] = round(time.monotonic() - t0, 2)
            pump(cdp, args.dwell)
            status = None
            for ev in cdp.events:
                p = ev.get("params") or {}
                if ev["method"] == "Network.responseReceived" and p.get("type") == "Document" \
                        and (main_frame is None or p.get("frameId") == main_frame):
                    status = int((p.get("response") or {}).get("status") or 0) or None
                elif ev["method"] == "Network.loadingFailed" and p.get("type") == "Document" \
                        and not snap.get("net_error"):
                    snap["net_error"] = p.get("errorText", "loadingFailed")
            snap["http_status"] = status
            exceptions = []
            for ev in cdp.events:
                p = ev.get("params") or {}
                if ev["method"] == "Runtime.exceptionThrown":
                    exceptions.append(chrome_exception_text(p))
                elif ev["method"] == "Runtime.consoleAPICalled" and p.get("type") == "error":
                    parts = [(a.get("value") if "value" in a else a.get("description", "")) for a in p.get("args") or []]
                    exceptions.append(" ".join(str(x) for x in parts if x is not None))
            snap["exceptions"] = [e for e in exceptions if e and not pa.NET_CONSOLE_RE.search(e)]
            snap.update(parse_probe(cdp.eval(PAGE_JS)))
            shot = cdp.call("Page.captureScreenshot", {"format": "png"}, timeout=30)
            out_png.write_bytes(base64.b64decode(shot["data"]))
            snap["png"] = str(out_png)
            snap["ax"] = flatten_chrome_ax(cdp.call("Accessibility.getFullAXTree", timeout=30).get("nodes") or [])
            snap["ok"] = bool(snap.get("ready")) and not snap.get("net_error") and status is not None
        except Exception as e:  # плечо одного сайта не должно ронять прогон
            snap["error"] = f"{type(e).__name__}: {e}"[:300]
        finally:
            for fn in (lambda: cdp.ws.sock.close() if cdp else None,
                       lambda: self.browser.call("Target.closeTarget", {"targetId": tid}) if tid else None,
                       lambda: self.browser.call("Target.disposeBrowserContext", {"browserContextId": ctx}) if ctx else None):
                try:
                    fn()
                except Exception:  # noqa: BLE001 — уборка best-effort
                    pass
        return snap


# ── Плечо Lumen ──────────────────────────────────────────────────────────────


def lumen_snapshot(br: "pa.LiveBrowser", url: str, args, out_png: Path) -> dict:
    snap: dict = {"arm": "lumen", "ok": False, "restarted": False}
    log_pos = br.log_path.stat().st_size if br.log_path.exists() else 0
    health_pos = br.health_log_pos
    ready = dead = hung = False
    net_failure = False
    br.hung.begin_site()
    t0 = time.monotonic()
    try:
        try:
            br.mcp.tool("new_tab", {"url": url})
        except RuntimeError:
            br.mcp.tool("navigate", {"url": url})
        try:
            br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": args.timeout * 1000})
            ready = True
        except RuntimeError as e:
            snap["error"] = str(e)[:200]
            if "command timed out" in str(e):
                hung = True
        snap["ready_s"] = round(time.monotonic() - t0, 2)
        if ready:
            try:
                br.mcp.tool("wait", {"condition": "network_idle", "timeout_ms": 5000})
            except RuntimeError:
                pass
            time.sleep(args.dwell)
            probe = parse_probe(br.mcp.tool("eval", {"code": PAGE_JS}).get("result"))
            snap.update(probe)
            shot = br.mcp.resource("resource://screenshot")
            if shot and shot[0].get("blob"):
                out_png.write_bytes(base64.b64decode(shot[0]["blob"]))
                snap["png"] = str(out_png)
            console = br.mcp.resource("resource://console")
            entries = json.loads(console[0].get("text", "[]")) if console else []
            errors = [e.get("message", "") for e in entries if e.get("level") == "Error"]
            snap["exceptions"] = [m for m in errors if not pa.NET_CONSOLE_RE.search(m)]
            tree = br.mcp.resource("resource://a11y_tree")
            snap["ax"] = flatten_lumen_ax(json.loads(tree[0]["text"])) if tree and tree[0].get("text") else None
    except (OSError, json.JSONDecodeError, socket.timeout) as e:
        dead = True
        snap["error"] = f"{type(e).__name__}: {e}"[:200]
    except RuntimeError as e:
        snap["error"] = str(e)[:200]
    chunk = br.stderr_since(log_pos)
    snap["http_status"] = lumen_final_status(chunk, url, snap.get("href"))
    snap["net_failures"] = pa.network_failures(chunk, url)
    # страница сетевой ошибки Lumen (net_error_page.rs) — «готова», с текстом и кадром, но это NET_FAIL, а не OK
    net_page = is_net_error_page(snap)
    net_failure = bool(snap["net_failures"]) or net_page
    stderr_errors, snap["stderr_error_sigs"], _ = br.stderr_errors_since(log_pos)
    snap["engine_counters"] = {k: v for k, v in pa.stderr_telemetry(chunk).items()
                               if k in ("se_net_fail", "se_panics", "se_warnings", "se_stack_overflow")}
    health, br.health_log_pos = br.health_events_since(health_pos)
    snap["status"] = pa.classify_status(
        dead=dead, hung=hung, http_status=snap["http_status"], net_failure=net_failure, ready=ready,
        broken_render=any(e.get("kind") == "broken_render" for e in health),
        have_png=bool(snap.get("png")), js_error_count=len(snap.get("exceptions") or []))
    if net_page and snap["status"] in ("OK", "DEGRADED", "BROKEN_RENDER"):
        snap["status"] = "NET_FAIL"
    snap["ok"] = ready and not dead and not hung
    if dead or hung or br.proc.poll() is not None:
        br.restart()
        snap["restarted"] = True
    return snap


def lumen_final_status(chunk: str, url: str, href: str | None) -> int | None:
    """Итоговый статус главного документа Lumen (проверка 1: «итоговый», не промежуточный).

    `pa.main_http_status(chunk, url)` берёт последний статус *запрошенного* URL, то есть 301/302
    редиректа на себя (http→https, `/` → `/int/`, cookie-редирект), а не конечной страницы; ищем
    по адресу, на котором страница остановилась. 304 (повторный прогон ревалидирует кэш главного
    документа) для Chrome выглядит как 200 из кэша — приводим к 200.
    """
    status = pa.main_http_status(chunk, href or url)
    return 200 if status == 304 else status


# ── PNG -> блоки ─────────────────────────────────────────────────────────────

_read_png = None


def load_frame(path: str | None) -> tuple | None:
    """PNG -> block_means; None, если кадра нет или он нечитаем."""
    global _read_png
    if not path or not Path(path).is_file():
        return None
    if _read_png is None:
        spec = importlib.util.spec_from_file_location("graphic_tests_run", REPO_ROOT / "graphic_tests" / "run.py")
        mod = importlib.util.module_from_spec(spec)
        sys.modules["graphic_tests_run"] = mod
        spec.loader.exec_module(mod)
        _read_png = mod.read_png
    try:
        w, h, bpp, px = _read_png(path)
    except Exception:  # noqa: BLE001 — битый PNG = «нет кадра»
        return None
    return block_means(w, h, bpp, px)


def png_width(path: str | None) -> int | None:
    try:
        with open(path, "rb") as f:
            head = f.read(24)
        return int.from_bytes(head[16:20], "big")
    except (OSError, TypeError):
        return None


# ── Сайт целиком ─────────────────────────────────────────────────────────────


def run_attempt(br, ref: ChromeRef, slug: str, url: str, args, out_dir: Path, attempt: int) -> dict:
    """Один прогон обоих плеч + пять проверок."""
    lumen = lumen_snapshot(br, url, args, out_dir / f"{slug}.lumen{attempt}.png")
    vp = lumen.get("viewport") or [args.viewport_w, args.viewport_h, 1]
    if not ref.alive():
        ref.restart()
    chrome = ref.snapshot(url, vp, args, out_dir / f"{slug}.chrome{attempt}.png")
    if not ref.alive():
        ref.restart()
    css_w = vp[0]
    for snap in (chrome, lumen):
        snap["frame"] = load_frame(snap.get("png"))
        w = png_width(snap.get("png"))
        snap["scale"] = (w / css_w) if (w and css_w) else 1.0
    # масштаб маски — по кадру Lumen (его размер определяет вьюпорт обоих плеч)
    chrome["scale"] = lumen["scale"] = lumen["scale"] or chrome["scale"]
    result = {"chrome": chrome, "lumen": lumen}
    if chrome["ok"] or chrome.get("http_status"):
        result["checks"] = evaluate(chrome, lumen, args)
    return result


def slim(snap: dict) -> dict:
    """Снимок для results.jsonl: без тяжёлых полей (кадр, текст, маски)."""
    out = {k: v for k, v in snap.items() if k not in ("frame", "text", "masks", "ax")}
    out["text_len"] = len(snap.get("text") or "")
    out["masks_n"] = len(snap.get("masks") or [])
    out["ax_n"] = None if snap.get("ax") is None else len(snap["ax"])
    return out


def measure_site(br, ref, slug: str, url: str, args, out_dir: Path) -> dict:
    rec: dict = {"slug": slug, "url": url, "started_at": datetime.now().isoformat(timespec="seconds")}
    first = run_attempt(br, ref, slug, url, args, out_dir, 1)
    runs = [first]
    if "checks" not in first:
        rec["ref_failed"] = True  # эталон не открыл сайт: паритет не с чем сравнивать
        rec["ref_error"] = first["chrome"].get("net_error") or first["chrome"].get("error")
    elif not all(c["ok"] for c in first["checks"].values()) and not args.no_rerun:
        runs.append(run_attempt(br, ref, slug, url, args, out_dir, 2))
    rec["reran"] = len(runs) > 1
    rec["runs"] = [{"chrome": slim(r["chrome"]), "lumen": slim(r["lumen"])} for r in runs]
    last = runs[-1]
    rec["lumen_status"] = last["lumen"].get("status")
    rec["computed"] = "checks" in first and all("checks" in r for r in runs)
    if rec["computed"]:
        second = runs[1]["checks"] if len(runs) > 1 else None
        rec["checks"], rec["flaps"] = combine_runs(first["checks"], second)
        rec["parity"] = all(c["ok"] for c in rec["checks"].values())
    rec["finished_at"] = datetime.now().isoformat(timespec="seconds")
    return rec


def verdict_of(rec: dict) -> str:
    """Короткий вердикт сайта для строки прогресса."""
    if rec.get("ref_failed"):
        return "REF-FAIL"
    if not rec["computed"]:
        return "NOT-COMPUTED"  # эталон не открыл сайт при повторе: проверки не сведены
    if rec["parity"]:
        return "PARITY"
    return "DIFF[" + ",".join(k for k in CHECKS if not rec["checks"][k]["ok"]) + "]"


# ── Отчёт ────────────────────────────────────────────────────────────────────


def summary_md(records: list[dict], meta: dict) -> str:
    s = summarize(records)
    lines = [
        "# Краулер паритета Lumen ↔ Chrome (COMPAT-CORPUS-1)", "",
        f"- коммит `{meta['commit']}`, lumen `{meta['exe']}`, Chrome `{meta['chrome']}`",
        f"- пороги: пиксели ≤ {meta['px_threshold']}% блоков ({BLOCK_PX}px, допуск {meta['px_tolerance']}), a11y ≥ {meta['ax_threshold'] * 100:.0f}%",
        "", "## Итог", "",
        f"- **CPR: {s['cpr_pct']}%** ({s['parity']} из {s['computed']} сайтов с посчитанными проверками)",
        f"- проверки посчитаны у {s['computed']} из {s['sites']} сайтов ({s['computed_pct']}%); эталон не открыл сайт: {s['ref_failed']}",
        f"- повторный прогон: {s['reran']} сайтов; флапы (разный исход проверки между прогонами): {s['flapped']} ({s['flap_pct']}%)",
        "", "| проверка | прошло | из |", "|---|---|---|",
    ]
    for k in CHECKS:
        lines.append(f"| {k} | {s['per_check_pass'][k]} | {s['computed']} |")
    computed = [r for r in records if r.get("computed")]
    sigs = Counter()
    for r in computed:
        for sig in r["checks"]["exceptions"].get("new_sigs", []):
            sigs[sig] += 1
    if sigs:
        lines += ["", "## Новые исключения по числу сайтов (автогруппировка)", ""]
        lines += [f"- {n}× `{sig}`" for sig, n in sigs.most_common(15)]
    walls = Counter(w for r in computed for w in r["checks"]["walls"].get("extra", []))
    if walls:
        lines += ["", "## Лишние стены", ""] + [f"- {n}× `{w}`" for w, n in walls.most_common(15)]
    statuses = Counter(r.get("lumen_status") for r in records)
    lines += ["", "## Статусы Lumen (таксономия perf_audit)", "",
              ", ".join(f"{k}: {v}" for k, v in statuses.most_common()), "",
              "## Сайты не в паритете", "", "| сайт | не прошло | детали |", "|---|---|---|"]
    for r in computed:
        failed = [k for k in CHECKS if not r["checks"][k]["ok"]]
        if failed:
            det = "; ".join(f"{k}: {r['checks'][k].get('detail', '')}"[:90] for k in failed)
            lines.append(f"| {r['slug']} | {', '.join(failed)} | {det.replace('|', '/')} |")
    refs = [r for r in records if r.get("ref_failed")]
    if refs:
        lines += ["", "## Эталон не открыл сайт (вне CPR)", ""]
        lines += [f"- {r['slug']}: {r.get('ref_error')}" for r in refs]
    flaps = [r for r in computed if r.get("flaps")]
    if flaps:
        lines += ["", "## Флапы", ""] + [f"- {r['slug']}: {', '.join(r['flaps'])}" for r in flaps]
    return "\n".join(lines) + "\n"


def read_results(path: Path) -> list[dict]:
    if not path.is_file():
        return []
    return [json.loads(ln) for ln in path.read_text(encoding="utf-8").splitlines() if ln.strip()]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--corpus", action="append", type=Path, help="файл корпуса `slug url` (повторяемый)")
    ap.add_argument("--only", default="", help="подстроки slug через запятую")
    ap.add_argument("--limit", type=int, default=0, help="не больше N сайтов")
    ap.add_argument("--exe", help="путь к lumen.exe")
    ap.add_argument("--chrome", help="путь к chrome.exe (по умолчанию Chrome for Testing из ms-playwright)")
    ap.add_argument("--headless", action="store_true", help="Chrome без окна (UA без HeadlessChrome)")
    ap.add_argument("--out", type=Path, help="каталог результата (по умолчанию .tmp/compat-crawl/<ts>)")
    ap.add_argument("--resume", type=Path, help="продолжить прогон в этом каталоге")
    ap.add_argument("--timeout", type=int, default=45, help="секунд на загрузку сайта в каждом плече")
    ap.add_argument("--dwell", type=float, default=3.0, help="пауза после готовности, секунд")
    ap.add_argument("--px-threshold", type=float, default=35.0, help="порог доли отличающихся блоков, %%")
    ap.add_argument("--px-tolerance", type=int, default=40, help="допуск по каналу при сравнении блоков")
    ap.add_argument("--ax-threshold", type=float, default=0.95)
    ap.add_argument("--viewport-w", type=int, default=1600)
    ap.add_argument("--viewport-h", type=int, default=900)
    ap.add_argument("--no-rerun", action="store_true", help="не перезапускать при расхождении")
    ap.add_argument("--restart-every", type=int, default=15, help="перезапуск окна Lumen каждые N сайтов")
    ap.add_argument("--env", action="append", default=[], metavar="KEY=VAL", help="доп. окружение для lumen")
    args = ap.parse_args()
    for kv in args.env:
        k, _, v = kv.partition("=")
        pa.EXTRA_ENV[k] = v

    only = [o for o in args.only.split(",") if o]
    sites, seen = [], set()
    for corpus in args.corpus or DEFAULT_CORPORA:
        try:
            part = pa.load_corpus(corpus, only)
        except SystemExit:  # фильтр --only ничего не нашёл в этом файле — в другом может быть
            continue
        for slug, url in part:
            if url not in seen:
                seen.add(url)
                sites.append((slug, url))
    if not sites:
        sys.exit(f"Корпус пуст (фильтр --only {only})")
    if args.limit:
        sites = sites[:args.limit]

    out_dir = args.resume or args.out or OUT_ROOT / datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir = out_dir.resolve()  # Chrome и lumen запускаются с другим cwd
    out_dir.mkdir(parents=True, exist_ok=True)
    results_path = out_dir / "results.jsonl"
    records = read_results(results_path)
    done = {r["slug"] for r in records}

    exe = pa.find_exe(args.exe)
    chrome_bin = args.chrome or cib.default_binary()
    if not chrome_bin:
        sys.exit("chrome.exe не найден: укажите --chrome или поставьте Chrome for Testing (ms-playwright)")
    commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"], capture_output=True, text=True,
                            cwd=REPO_ROOT).stdout.strip()
    meta = {"commit": commit, "exe": str(exe), "chrome": cib.chromium_version(chrome_bin),
            "px_threshold": args.px_threshold, "px_tolerance": args.px_tolerance, "ax_threshold": args.ax_threshold}
    print(f"[compat-crawl] {len(sites)} сайтов, уже готово {len(done)}, вывод {out_dir}", flush=True)

    ref = ChromeRef(chrome_bin, args.headless, out_dir)
    br = pa.LiveBrowser(exe, out_dir, args.timeout)
    try:
        for i, (slug, url) in enumerate(sites, 1):
            if slug in done:
                continue
            if args.restart_every and len(done) and i % args.restart_every == 0:
                br.restart()
            rec = measure_site(br, ref, slug, url, args, out_dir)
            records.append(rec)
            done.add(slug)
            with results_path.open("a", encoding="utf-8") as f:
                f.write(json.dumps(rec, ensure_ascii=False) + "\n")
            verdict = verdict_of(rec)
            print(f"[{i}/{len(sites)}] {slug}: {verdict} lumen={rec.get('lumen_status')}", flush=True)
    finally:
        br.close()
        ref.close()
    (out_dir / "summary.md").write_text(summary_md(records, meta), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summarize(records), ensure_ascii=False, indent=1), encoding="utf-8")
    s = summarize(records)
    print(f"[compat-crawl] CPR {s['cpr_pct']}% · посчитано {s['computed']}/{s['sites']} · флапы {s['flap_pct']}% · {out_dir / 'summary.md'}")


if __name__ == "__main__":
    main()
