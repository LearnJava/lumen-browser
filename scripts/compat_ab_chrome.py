#!/usr/bin/env python3
"""Плечо «стек Chrome» A/B-прогона идентичности (COMPAT-AB-IDENTITY-HARNESS).

Нужно, чтобы развести два фактора, которые в одном прогоне Lumen слипаются:
*стек* (TLS, H2, порядок заголовков, движок) и *UA-токен* (`Lumen/x.y` в UA и
бренд `Lumen` в Sec-CH-UA). Настоящий Chrome for Testing той же мажорной
версии, что и `chrome_major!` в `crates/core/src/identity.rs`, ходит на те же
сайты с двумя вариантами идентичности:

  * `plain` — UA стоковый Chrome (`chrome_unbranded` у Lumen);
  * `lumen` — тот же UA + ` Lumen/<версия>` и бренд `Lumen` в Client Hints
    (`chrome_lumen` у Lumen, ADR-034).

Остальной отпечаток не трогается. Профиль на каждый сайт и вариант свежий;
каждый сайт — отдельный процесс Chrome (как `--mode compat` у `perf_audit.py`).
Исход сайта — по таксономии `perf_audit.classify_status` плюс стены из
`wall_signatures.py`: `SITE_REFUSED` (отказ + подпись вендора), `WALL_SOFT`
(200, но заглушка «Unsupported browser»), `NET_FAIL`, `TIMEOUT`, `EMPTY`, `SLOW_LOAD`
(200 и DOM есть, но `load` не пришёл), `OK`.

    python scripts/compat_ab_chrome.py --fetch-cft            # скачать CfT нужного мажора
    python scripts/compat_ab_chrome.py --corpus docs/perf/corpus.txt --variant plain --variant lumen
    python scripts/compat_ab_chrome.py --url khanacademy=https://www.khanacademy.org/ --ua-major 130

Результат: `<out>/results.json` (список записей `slug`/`url`/`variant`/`status`/
`http_status`/`wall`/…), `<out>/summary.md`. `--json PATH` копирует results.json
в `docs/perf/runs/` под нужным именем. Нужен Python ≥ 3.10, только stdlib.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request
import zipfile
from datetime import datetime
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from bench_chromium import WS, free_port  # noqa: E402
from header_echo_server import EchoServer  # noqa: E402
from wall_signatures import classify_wall  # noqa: E402

REPO = Path(__file__).resolve().parent.parent

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

CFT_INDEX = "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json"
CFT_ROOT = Path(os.environ.get("LUMEN_CFT_DIR") or REPO / ".tmp" / "cft")
UA_TEMPLATE = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
)
ACCEPT_LANGUAGE = "en-US,en;q=0.9"  # то же, что у Lumen (DEFAULT_ACCEPT_LANGUAGE)
# Chrome сам дописывает q-значения к списку языков: в CDP-override отдаём без них.
CDP_LANGUAGES = "en-US,en"
VARIANTS = ("plain", "lumen")
EMPTY_NODES = 15  # меньше узлов в DOM — страница пуста
BLOCKED = {"SITE_REFUSED", "WALL_SOFT", "NET_FAIL", "HTTP_ERROR"}  # исходы «сайт не пустил»
MAX_BODY = 400_000


# ── версии ───────────────────────────────────────────────────────────────────

def lumen_chrome_major() -> str:
    """Мажор Chrome из `chrome_major!()` — единый источник версии (CLAUDE.md)."""
    src = (REPO / "crates" / "core" / "src" / "identity.rs").read_text(encoding="utf-8")
    m = re.search(r"macro_rules!\s+chrome_major\s*\{\s*\(\)\s*=>\s*\{\s*\"(\d+)\"", src)
    if not m:
        sys.exit("chrome_major! не найден в crates/core/src/identity.rs")
    return m.group(1)


def lumen_version() -> str:
    """Версия браузера — `[workspace.package] version` (то, что даёт CARGO_PKG_VERSION)."""
    toml = (REPO / "Cargo.toml").read_text(encoding="utf-8")
    m = re.search(r"^version\s*=\s*\"([^\"]+)\"", toml, re.M)
    if not m:
        sys.exit("version не найден в Cargo.toml")
    return m.group(1)


def user_agent(variant: str, major: str, lumen_ver: str) -> str:
    ua = UA_TEMPLATE.format(major=major)
    return f"{ua} Lumen/{lumen_ver}" if variant == "lumen" else ua


# ── Chrome for Testing ───────────────────────────────────────────────────────

def chrome_binary_version(binary: Path) -> str:
    # `chrome.exe --version` на Windows открывает окно; версия — имя `<версия>.manifest`.
    for name in os.listdir(binary.parent):
        if name.endswith(".manifest") and name[0].isdigit():
            return name[: -len(".manifest")]
    return "unknown"


def find_chrome(explicit: str | None, major: str) -> tuple[Path, str]:
    """Бинарник Chrome: --binary > $CHROME_BIN > CfT-кэш нужного мажора > Playwright."""
    if explicit or os.environ.get("CHROME_BIN"):
        p = Path(explicit or os.environ["CHROME_BIN"])
        if not p.is_file():
            sys.exit(f"нет бинарника: {p}")
        return p, chrome_binary_version(p)
    cands = sorted(glob.glob(str(CFT_ROOT / f"{major}.*" / "chrome-win64" / "chrome.exe")))
    if cands:
        p = Path(cands[-1])
        return p, chrome_binary_version(p)
    sys.exit(
        f"нет Chrome for Testing {major}.x в {CFT_ROOT}. Скачать: "
        "python scripts/compat_ab_chrome.py --fetch-cft  (или --binary путь/к/chrome.exe)"
    )


def fetch_cft(major: str) -> Path:
    """Скачать последний CfT win64 с нужным мажором в `CFT_ROOT/<версия>/`."""
    with urllib.request.urlopen(CFT_INDEX, timeout=60) as r:
        index = json.load(r)
    matches = [v for v in index["versions"] if v["version"].split(".")[0] == major and v.get("downloads", {}).get("chrome")]
    if not matches:
        have = sorted({v["version"].split(".")[0] for v in index["versions"]}, key=int)[-5:]
        sys.exit(f"в Chrome for Testing нет мажора {major}; последние: {', '.join(have)}")
    ver = matches[-1]
    url = next(d["url"] for d in ver["downloads"]["chrome"] if d["platform"] == "win64")
    dest = CFT_ROOT / ver["version"]
    exe = dest / "chrome-win64" / "chrome.exe"
    if exe.is_file():
        print(f"CfT {ver['version']} уже есть: {exe}")
        return exe
    dest.mkdir(parents=True, exist_ok=True)
    zpath = dest / "chrome-win64.zip"
    print(f"качаю {url} → {zpath}", flush=True)
    urllib.request.urlretrieve(url, zpath)
    with zipfile.ZipFile(zpath) as z:
        z.extractall(dest)
    zpath.unlink()
    print(f"CfT {ver['version']}: {exe}")
    return exe


# ── CDP ──────────────────────────────────────────────────────────────────────

class Page:
    """Одна вкладка Chrome по CDP. События копятся и разбираются в `pump`."""

    def __init__(self, ws: WS) -> None:
        self.ws = ws
        self.n = 0
        self.main_req: dict = {}
        self.docs: dict[str, dict] = {}
        self.requests = 0
        self.failed = 0
        self.loaded = False
        self.frame_id: str | None = None
        self.extra_req_headers: dict[str, dict] = {}
        self.extra_resp_headers: dict[str, dict] = {}

    def _event(self, msg: dict) -> None:
        m, p = msg["method"], msg.get("params", {})
        if m == "Network.requestWillBeSent":
            self.requests += 1
            if p.get("type") == "Document" and p.get("frameId") == self.frame_id:
                self.docs[p["requestId"]] = {"url": p["request"]["url"], "request_headers": p["request"].get("headers", {})}
        elif m == "Network.requestWillBeSentExtraInfo":
            self.extra_req_headers[p["requestId"]] = p.get("headers", {})
        elif m == "Network.responseReceivedExtraInfo":
            self.extra_resp_headers[p["requestId"]] = {"status": p.get("statusCode"), "headers": p.get("headers", {})}
        elif m == "Network.responseReceived" and p["requestId"] in self.docs:
            r = p["response"]
            self.docs[p["requestId"]].update(
                status=r["status"], resp_headers=r.get("headers", {}), mime=r.get("mimeType"), protocol=r.get("protocol"), url=r["url"]
            )
        elif m == "Network.loadingFailed":
            self.failed += 1
            if p["requestId"] in self.docs:
                self.docs[p["requestId"]]["error"] = p.get("errorText")
        elif m == "Page.loadEventFired":
            self.loaded = True

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
            except socket.timeout:
                break
            if msg.get("id") == mid:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})
            if "method" in msg:
                self._event(msg)
        raise TimeoutError(method)

    def pump(self, seconds: float, until=None) -> None:
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if until and until():
                return
            self.ws.sock.settimeout(0.3)
            try:
                msg = json.loads(self.ws.recv())
            except socket.timeout:
                continue
            if "method" in msg:
                self._event(msg)

    def eval(self, expr: str, timeout: float = 20):
        r = self.call("Runtime.evaluate", {"expression": expr, "returnByValue": True, "awaitPromise": True}, timeout)
        return r.get("result", {}).get("value")


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
    """Процесс Chrome со свежим профилем и вариантом идентичности."""

    def __init__(self, binary: Path, variant: str, major: str, lumen_ver: str, visible: bool, ua_major: str | None) -> None:
        self.variant = variant
        self.ua = user_agent(variant, ua_major or major, lumen_ver)
        self.lumen_major = lumen_ver.split(".")[0]
        self.profile = tempfile.mkdtemp(prefix="ab-chrome-")
        # Холодный старт Chrome изредка не открывает CDP-порт (2026-10-13: 2 из 14 запусков при
        # нагруженной машине; порт могли занять между free_port() и запуском): до трёх попыток,
        # каждая на новом порту.
        for attempt in (1, 2, 3):
            self.port = free_port()
            args = [
                str(binary), f"--remote-debugging-port={self.port}", f"--user-data-dir={self.profile}",
                "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--window-size=1366,768",
                "--disable-blink-features=AutomationControlled", f"--lang={ACCEPT_LANGUAGE.split(',')[0]}",
                "about:blank",
            ]
            if not visible:
                args.insert(1, "--headless=new")
            self.proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                self.page = Page(WS(page_ws_url(self.port, timeout=30)))
                break
            except (RuntimeError, OSError):
                subprocess.run(["taskkill", "/PID", str(self.proc.pid), "/T", "/F"], capture_output=True)
                if attempt == 3:
                    raise

    def setup(self) -> None:
        p = self.page
        p.call("Page.enable")
        p.call("Network.enable")
        p.frame_id = p.call("Page.getFrameTree")["frameTree"]["frame"]["id"]  # документы iframe не считаем главным
        # Бренды берём у самого бинарника (CfT: Chromium + GREASE), убираем HeadlessChrome,
        # для варианта `lumen` добавляем `Lumen` — как Lumen в Sec-CH-UA (client_hints.rs).
        # Флаг `--user-agent` использовать нельзя: с ним Chrome обнуляет Client Hints
        # (захват 2026-10-13: `sec-ch-ua` пустой), поэтому UA и метаданные — одним CDP-override.
        # `userAgentData` есть только в защищённом контексте: about:blank его не даёт, loopback — даёт.
        with EchoServer() as echo:
            p.call("Page.navigate", {"url": echo.url + "/"})
            p.pump(15, until=lambda: p.loaded)
            ver = p.eval(
                "navigator.userAgentData.getHighEntropyValues(['fullVersionList','platformVersion','architecture','bitness'])"
                ".then(h => ({b: navigator.userAgentData.brands, f: h.fullVersionList, pv: h.platformVersion, a: h.architecture, bt: h.bitness}))"
            ) or {}
        if not ver.get("b"):
            raise RuntimeError("navigator.userAgentData.brands пуст: бренды Chrome не получены")
        p.loaded = False
        p.docs.clear()
        p.requests = p.failed = 0
        brands = [b for b in ver.get("b", []) if "Headless" not in b["brand"]]
        full = [b for b in ver.get("f", []) if "Headless" not in b["brand"]]
        if self.variant == "lumen":
            brands.append({"brand": "Lumen", "version": self.lumen_major})
            full.append({"brand": "Lumen", "version": self.lumen_major + ".0.0.0"})
        p.call("Emulation.setUserAgentOverride", {
            "userAgent": self.ua, "acceptLanguage": CDP_LANGUAGES,
            "userAgentMetadata": {
                "brands": brands, "fullVersionList": full, "platform": "Windows",
                "platformVersion": ver.get("pv", "10.0.0"), "architecture": ver.get("a", "x86"),
                "bitness": ver.get("bt", "64"), "model": "", "mobile": False,
            },
        })

    def close(self) -> None:
        try:
            self.page.ws.sock.close()
        except OSError:
            pass
        subprocess.run(["taskkill", "/PID", str(self.proc.pid), "/T", "/F"], capture_output=True)
        for _ in range(10):  # профиль освобождается не сразу
            try:
                shutil.rmtree(self.profile)
                break
            except OSError:
                time.sleep(0.5)


# ── прогон сайта ─────────────────────────────────────────────────────────────

def audit_site(binary: Path, chrome_ver: str, slug: str, url: str, variant: str, args, major: str, lumen_ver: str) -> dict:
    rec: dict = {"slug": slug, "url": url, "variant": variant, "chrome_version": chrome_ver, "ua": None}
    sess = None
    t0 = time.monotonic()
    try:
        sess = ChromeSession(binary, variant, major, lumen_ver, args.visible, args.ua_major)
        rec["ua"] = sess.ua
        sess.setup()
        page = sess.page
        nav = page.call("Page.navigate", {"url": url}, timeout=30)
        rec["navigation_error"] = nav.get("errorText")
        page.pump(args.timeout, until=lambda: page.loaded or rec["navigation_error"])
        rec["load_ms"] = int((time.monotonic() - t0) * 1000) if page.loaded else None
        page.pump(args.settle)  # поздние редиректы и JS-заглушки
        docs = [d for d in page.docs.values() if "status" in d or "error" in d]
        main = docs[-1] if docs else {}
        rid = next((k for k, v in page.docs.items() if v is main), None)
        rec["http_status"] = main.get("status")
        rec["protocol"] = main.get("protocol")
        rec["final_url"] = main.get("url")
        rec["net_error"] = main.get("error") or rec["navigation_error"]
        rec["request_headers"] = page.extra_req_headers.get(rid) or main.get("request_headers", {})
        body = ""
        if rid and main.get("status"):
            try:
                r = page.call("Network.getResponseBody", {"requestId": rid}, timeout=15)
                body = r.get("body", "")[:MAX_BODY]
            except (RuntimeError, TimeoutError):
                pass
        dom = page.eval(
            "({href: location.href, title: document.title, nodes: document.getElementsByTagName('*').length,"
            " text: (document.body ? document.body.innerText : '').slice(0, 20000),"
            " ua: navigator.userAgent, webdriver: navigator.webdriver})"
        ) or {}
        rec.update(title=dom.get("title"), dom_nodes=dom.get("nodes"), page_ua=dom.get("ua"), requests=page.requests, failed_requests=page.failed)
        wall = classify_wall(
            rec["http_status"], main.get("resp_headers"), body, net_error=rec["net_error"], visible_text=dom.get("text") or ""
        )
        rec["wall"] = wall
        rec["status"] = classify(rec, wall, page.loaded)
    except Exception as exc:  # один сбойный сайт не должен ронять корпус
        rec["status"] = "ERROR"
        rec["error"] = f"{type(exc).__name__}: {exc}"
    finally:
        if sess:
            sess.close()
    rec["wall_s"] = round(time.monotonic() - t0, 1)
    return rec


def classify(rec: dict, wall: dict, loaded: bool) -> str:
    st = rec.get("http_status")
    if st is None:
        return "NET_FAIL" if rec.get("net_error") else "TIMEOUT"
    if wall["kind"] in ("hard", "refused") or st in (401, 403, 429):
        return "SITE_REFUSED"
    if wall["kind"] == "soft":
        return "WALL_SOFT"
    if st >= 400:
        return "HTTP_ERROR"
    if (rec.get("dom_nodes") or 0) < EMPTY_NODES:
        return "EMPTY" if loaded else "TIMEOUT"
    # 200, DOM есть, но `load` не пришёл за --timeout (реклама, трекеры): страницу
    # сайт отдал, к отказу из-за идентичности это не относится.
    return "OK" if loaded else "SLOW_LOAD"


# ── отчёт ────────────────────────────────────────────────────────────────────

def summary_md(results: list[dict], variants: list[str], meta: dict) -> str:
    out = [
        "# Плечо «стек Chrome»: A/B идентичности",
        "",
        f"Chrome {meta['chrome_version']} (мажор Lumen: {meta['lumen_major']}), UA-мажор: {meta['ua_major']}, "
        f"Lumen/{meta['lumen_version']}, {meta['date']}.",
        "",
        "| Вариант | " + " | ".join(sorted({r['status'] for r in results})) + " |",
        "|---|" + "---|" * len({r["status"] for r in results}),
    ]
    statuses = sorted({r["status"] for r in results})
    for v in variants:
        rs = [r for r in results if r["variant"] == v]
        out.append(f"| {v} ({len(rs)}) | " + " | ".join(str(sum(1 for r in rs if r["status"] == s)) for s in statuses) + " |")
    out += ["", "| Сайт | " + " | ".join(variants) + " | Стена |", "|---|" + "---|" * (len(variants) + 1)]
    slugs = list(dict.fromkeys(r["slug"] for r in results))
    for slug in slugs:
        cells, walls = [], []
        for v in variants:
            r = next((x for x in results if x["slug"] == slug and x["variant"] == v), None)
            if not r:
                cells.append("—")
                continue
            cells.append(f"{r['status']} ({r.get('http_status')})")
            w = r.get("wall") or {}
            if w.get("kind") not in (None, "none"):
                walls.append(f"{v}: {w['kind']}/{w.get('vendor')}")
        out.append(f"| {slug} | " + " | ".join(cells) + f" | {'; '.join(walls) or '—'} |")
    # Токен «виноват», когда в одном варианте сайт пустил, а в другом заблокировал.
    diff = [s for s in slugs if len({r["status"] in BLOCKED for r in results if r["slug"] == s}) > 1]
    if len(variants) > 1:
        out += ["", f"Сайтов, где блокировка зависит от токена: {len(diff)} из {len(slugs)}" + (f" — {', '.join(diff)}" if diff else "")
                + f". Блокировка = {', '.join(sorted(BLOCKED))}; SLOW_LOAD/TIMEOUT в счёт не идут."]
    return "\n".join(out) + "\n"


def load_corpus(path: Path, only: list[str]) -> list[tuple[str, str]]:
    sites = []
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        slug, url = line.split(None, 1)
        if only and not any(o in slug for o in only):
            continue
        sites.append((slug, url.strip()))
    return sites


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--corpus", type=Path, help="файл `slug url` (как docs/perf/corpus.txt)")
    ap.add_argument("--url", action="append", default=[], metavar="SLUG=URL", help="один сайт; можно несколько раз")
    ap.add_argument("--only", action="append", default=[], help="фильтр корпуса по подстроке slug")
    ap.add_argument("--variant", action="append", choices=VARIANTS, help="вариант идентичности (по умолчанию оба)")
    ap.add_argument("--ua-major", help="мажор Chrome в UA вместо chrome_major! (проверка «устаревший UA», BUG-1113)")
    ap.add_argument("--binary", help="chrome.exe; по умолчанию CfT нужного мажора из .tmp/cft")
    ap.add_argument("--fetch-cft", action="store_true", help="скачать Chrome for Testing мажора chrome_major! и выйти")
    ap.add_argument("--timeout", type=float, default=45, help="секунд на загрузку страницы")
    ap.add_argument("--settle", type=float, default=3, help="секунд после load на поздние редиректы/заглушки")
    ap.add_argument("--visible", action="store_true", help="окно вместо headless=new")
    ap.add_argument("--out-dir", type=Path)
    ap.add_argument("--json", type=Path, help="скопировать results.json сюда (docs/perf/runs/<дата>-ab-chrome.json)")
    args = ap.parse_args()

    major = lumen_chrome_major()
    if args.fetch_cft:
        fetch_cft(major)
        return 0

    sites = [tuple(u.split("=", 1)) for u in args.url]
    if args.corpus:
        sites += load_corpus(args.corpus, args.only)
    if not sites:
        ap.error("нужен --corpus или --url")
    variants = args.variant or list(VARIANTS)
    binary, chrome_ver = find_chrome(args.binary, major)
    lumen_ver = lumen_version()
    mismatch = chrome_ver.split(".")[0] != major
    if mismatch:
        print(f"ВНИМАНИЕ: мажор бинарника {chrome_ver} ≠ chrome_major! {major}: Client Hints и движок не те", file=sys.stderr)

    out_dir = args.out_dir or REPO / ".tmp" / "compat-ab-chrome" / datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir.mkdir(parents=True, exist_ok=True)
    meta = {
        "chrome_version": chrome_ver, "lumen_major": major, "ua_major": args.ua_major or major, "lumen_version": lumen_ver,
        "date": datetime.now().strftime("%Y-%m-%d"), "major_mismatch": mismatch, "headless": not args.visible,
    }
    results: list[dict] = []
    for slug, url in sites:
        for variant in variants:
            rec = audit_site(binary, chrome_ver, slug, url, variant, args, major, lumen_ver)
            results.append(rec)
            w = rec.get("wall") or {}
            print(f"{slug:<22} {variant:<6} {rec['status']:<13} http={rec.get('http_status')} wall={w.get('kind')}/{w.get('vendor')} "
                  f"nodes={rec.get('dom_nodes')} {rec['wall_s']}s", flush=True)
            (out_dir / "results.json").write_text(json.dumps({"meta": meta, "results": results}, ensure_ascii=False, indent=1), encoding="utf-8")
    (out_dir / "summary.md").write_text(summary_md(results, variants, meta), encoding="utf-8")
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(out_dir / "results.json", args.json)
    print(f"\n{out_dir / 'summary.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
