#!/usr/bin/env python3
"""Стенд G (COMPAT-CORPUS-4): страницы капч и антибот-проверок, вердикт на страницу и GPR.

Каждая страница корпуса `docs/perf/corpus-captcha.tsv` открывается в свежем процессе
браузера (свежий профиль), затем до `--observe-s` секунд раз в `--poll-s` секунд снимается
одно и то же состояние страницы JS-зондом. Капчи НЕ решаются: ни клика, ни ввода.

Вердикт на страницу (по ROADMAP COMPAT-CORPUS-4):
  пройдено       — выполнен оракул страницы (token / render / score / unblocked, см. шапку корпуса)
  петля          — документ перезагружен ≥3 раз за прогон (тот же challenge ≥3 раз в минуту),
                   оракул не выполнен
  не_отрисован   — страница загрузилась, но виджет вендора не появился / токена нет / балла нет
  отказ          — навигация не удалась или страница показала отказ (access denied, unsupported
                   browser, 403 …)
  завис          — страница-проверка не ушла за окно наблюдения и не перезагружалась (challenge
                   без виджета, JS-проверка не завершилась)

GPR = доля страниц с вердиктом «пройдено». Балл reCAPTCHA v3 — из страницы `score`.

Два плеча (один и тот же зонд и оракулы):
    python scripts/captcha_stand.py --arm lumen  [--exe lumen.exe] [--env KEY=VAL ...]
    python scripts/captcha_stand.py --arm chrome [--binary chrome.exe]   # эталон: Chromium из кэша Playwright

    --only id1,id2     часть корпуса
    --out PATH.json    результат (по умолчанию .tmp/captcha-stand/<дата>-<плечо>.json)
    --baseline PATH    сравнить GPR с прошлым JSON того же плеча; выход 1, если «пройдено» стало
                       меньше у какой-либо страницы (для ночного прогона и блокировки релиза)
    --compare PATH     сравнить с JSON другого плеча (Lumen против Chrome) и вывести расхождения

Окно живое (`--maximized`, блокировщик рекламы выключен): прогон идёт по настоящей сети, результат
зависит от IP и маршрута — поле `route` фиксирует метку (`--route`), сравнивать только прогоны
одного маршрута.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.parse
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import perf_audit  # noqa: E402  (LiveBrowser, find_exe, EXTRA_ENV)
from bench_chromium import WS, free_port  # noqa: E402
from chrome_interaction_baseline import Cdp, chromium_version, default_binary, page_ws_url  # noqa: E402

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_CORPUS = REPO_ROOT / "docs" / "perf" / "corpus-captcha.tsv"
OUT_ROOT = REPO_ROOT / ".tmp" / "captcha-stand"

VERDICTS = ("пройдено", "петля", "не_отрисован", "отказ", "завис")
LOOP_LOADS = 3  # столько загрузок документа за прогон = петля

# iframe виджета по вендору (хост/путь в src)
FRAME_RE = {
    "turnstile": re.compile(r"challenges\.cloudflare\.com", re.I),
    "cf": re.compile(r"challenges\.cloudflare\.com", re.I),
    "recaptcha": re.compile(r"(google\.com|recaptcha\.net)/recaptcha", re.I),
    "hcaptcha": re.compile(r"hcaptcha\.com", re.I),
    "smartcaptcha": re.compile(r"smartcaptcha|captcha\.yandex|yandexcloud\.net", re.I),
}
CHALLENGE_RE = re.compile(
    r"just a moment|checking your browser|checking if the site connection is secure|verify you are human"
    r"|verifying you are human|making sure you.re not a bot|performing security verification"
    r"|attention required|are you a robot|проверка браузера|проверяем ваш браузер|подтвердите, что вы не робот"
    r"|ddos-guard.{0,40}(check|провер)|checking your browser before accessing",
    re.I,
)
DENY_RE = re.compile(
    r"access denied|you have been blocked|sorry, you have been blocked|unsupported browser|browser is not supported"
    r"|403 forbidden|error 1020|request blocked|pardon our interruption|доступ запрещ|доступ ограничен"
    r"|ваш браузер не поддерживается|incident id|reference #",
    re.I,
)
# страница сетевой ошибки самого браузера (net_error_page.rs у Lumen, neterror у Chromium): не «пройдено»,
# хотя текста на ней ≥200 символов и нет ни challenge, ни отказа сайта
NET_ERROR_RE = re.compile(
    r"слишком долго не отвечает|отклонил подключение|соединение с сайтом .{0,80} прервано|не удаётся добраться до сайта"
    r"|зациклил перенаправления|прислал непонятный ответ|не удалось установить защищённое соединение"
    r"|this site can.t be reached|err_[a-z_]{6,}|took too long to respond",
    re.I,
)
SCORE_RE = re.compile(r'"score"\s*:\s*"?([01](?:\.\d+)?)', re.I)
VENDOR_COOKIE_RE = re.compile(
    r"^(datadome|_abck|bm_sz|ak_bmsc|incap_ses_|visid_incap_|__ddg|qrator_jsid|techaro\.lol-anubis|_px|cf_clearance|__cf_bm)",
    re.I,
)

PROBE_JS = r"""(function(){
 var d=document,b=d.body;
 if(!window.__cs_mark){window.__cs_mark=Math.random().toString(36).slice(2);}
 /* Демо-страница v3 сама вызывает execute+verify, но в Chrome её fetch не уходит (проверено через CDP):
    повторяем те же два вызова скриптом демо один раз, чтобы получить балл; капча не решается. */
 if(!window.__cs_v3&&/\/recaptcha-v3-request-scores\.php$/.test(location.pathname)&&window.grecaptcha&&typeof grecaptcha.execute==='function'){
   window.__cs_v3='pending';
   try{grecaptcha.ready(function(){grecaptcha.execute('6LdKlZEpAAAAAAOQjzC2v_d36tWxCl6dWsozdSy9',{action:'examples/v3scores'})
     .then(function(t){return fetch('/recaptcha-v3-verify.php?action=examples/v3scores&token='+t).then(function(r){return r.text();}).then(function(x){window.__cs_v3=x;});})
     .catch(function(e){window.__cs_v3='ERR '+e;});});}catch(e){window.__cs_v3='ERR '+e;}
 }
 var frames=[].map.call(d.querySelectorAll('iframe'),function(f){var r=f.getBoundingClientRect();
   return {src:String(f.src||'').slice(0,200),w:Math.round(r.width),h:Math.round(r.height)};});
 var tok={};
 ['cf-turnstile-response','g-recaptcha-response','h-captcha-response','smart-token'].forEach(function(n){
   var e=d.querySelector('[name="'+n+'"]');if(e){tok[n]=String(e.value||'').length;}});
 var text=b?String(b.innerText!==undefined?b.innerText:b.textContent||''):'';
 return JSON.stringify({href:String(location.href),rs:String(d.readyState),title:String(d.title||''),
   mark:window.__cs_mark,text:text.slice(0,1500),text_len:text.length,frames:frames,tok:tok,
   v3:String(window.__cs_v3||'').slice(0,400),
   api:{turnstile:typeof window.turnstile,grecaptcha:typeof window.grecaptcha,hcaptcha:typeof window.hcaptcha},
   cookies:String(d.cookie||'').split(';').map(function(s){return s.split('=')[0].trim();}).filter(Boolean).slice(0,60)});
})()"""


# ── корпус ───────────────────────────────────────────────────────────────────

def load_corpus(path: Path, only: set[str]) -> list[dict]:
    pages = []
    for ln in path.read_text(encoding="utf-8").splitlines():
        if not ln.strip() or ln.startswith("#"):
            continue
        cols = ln.split("\t")
        if len(cols) < 4:
            sys.exit(f"{path}: строка без 4 колонок: {ln[:80]!r}")
        page = {"id": cols[0], "vendor": cols[1], "oracle": cols[2], "url": cols[3], "note": cols[4] if len(cols) > 4 else ""}
        if page["oracle"] not in ("token", "render", "score", "unblocked"):
            sys.exit(f"{path}: неизвестный оракул {page['oracle']!r} у {page['id']}")
        if not only or page["id"] in only:
            pages.append(page)
    if only and {p["id"] for p in pages} != only:
        sys.exit(f"--only: нет в корпусе: {sorted(only - {p['id'] for p in pages})}")
    return pages


# ── зонд и оракулы (общие для обоих плеч) ────────────────────────────────────

def decode_probe(val) -> dict | None:
    for _ in range(3):  # JSON-строка могла прийти дважды сериализованной
        if isinstance(val, str):
            try:
                val = json.loads(val)
            except json.JSONDecodeError:
                return None
        else:
            break
    return val if isinstance(val, dict) else None


def is_challenge(p: dict) -> bool:
    return bool(CHALLENGE_RE.search(p.get("title", "") + "\n" + p.get("text", "")[:600]))


def is_denied(p: dict) -> bool:
    return bool(DENY_RE.search(p.get("title", "") + "\n" + p.get("text", "")[:600]))


def is_net_error(p: dict) -> bool:
    return bool(NET_ERROR_RE.search(p.get("title", "") + "\n" + p.get("text", "")[:400]))


def widget_frames(page: dict, p: dict) -> list[dict]:
    rx = FRAME_RE.get(page["vendor"])
    if rx is None:
        return []
    return [f for f in p.get("frames", []) if rx.search(f["src"]) and f["w"] >= 20 and f["h"] >= 20]


def oracle_met(page: dict, p: dict) -> tuple[bool, dict]:
    """Выполнен ли оракул страницы по текущему состоянию p; extra — измеренное (балл, токен)."""
    kind = page["oracle"]
    if kind == "token":
        lens = {k: v for k, v in p.get("tok", {}).items() if v > 0}
        return bool(lens), {"token_fields": lens}
    if kind == "render":
        fr = widget_frames(page, p)
        return bool(fr), {"widget_frames": [{"host": urllib.parse.urlsplit(f["src"]).netloc, "w": f["w"], "h": f["h"]} for f in fr[:3]]}
    if kind == "score":
        m = SCORE_RE.search(p.get("v3", "")) or SCORE_RE.search(p.get("text", ""))
        return (m is not None), ({"v3_score": float(m.group(1))} if m else {})
    # unblocked: итоговая страница не challenge, не отказ и не пустая
    ok = p.get("rs") == "complete" and p.get("text_len", 0) >= 200 and not is_challenge(p) and not is_denied(p) and not is_net_error(p)
    return ok, {}


def observe(page: dict, probe, observe_s: float, poll_s: float, settle_s: float) -> dict:
    """Цикл наблюдения: probe() → dict|None. Возвращает запись с вердиктом (без метаданных плеча)."""
    marks: list[str] = []
    last: dict = {}
    extra: dict = {}
    met_at: float | None = None
    probe_errors = 0
    t0 = time.monotonic()
    while time.monotonic() - t0 < observe_s:
        p = probe()
        if p is None:
            probe_errors += 1
        else:
            last = p
            if p.get("mark") and (not marks or marks[-1] != p["mark"]):
                marks.append(p["mark"])
            met, ex = oracle_met(page, p)
            if met and p.get("rs") == "complete":
                extra = ex
                if met_at is None:
                    met_at = time.monotonic() - t0
                # оракул держится settle_s: страница не должна тут же уйти в петлю
                if time.monotonic() - t0 - met_at >= settle_s:
                    break
            else:
                met_at = None
                if is_denied(p) and p.get("rs") == "complete" and not is_challenge(p) and page["oracle"] != "unblocked":
                    # отказ на странице с виджетом ждать нет смысла
                    break
        time.sleep(poll_s)
    elapsed = round(time.monotonic() - t0, 1)
    loads = len(marks)
    rec = {"observed_s": elapsed, "loads": loads, "probe_errors": probe_errors}
    if last:
        rec.update({
            "final_url": last.get("href"), "title": last.get("title", "")[:120], "text_len": last.get("text_len"),
            "text_head": last.get("text", "")[:600], "api": last.get("api"),
            "vendor_cookies": sorted({c for c in last.get("cookies", []) if VENDOR_COOKIE_RE.match(c)}),
            "frames": [urllib.parse.urlsplit(f["src"]).netloc for f in last.get("frames", [])][:8],
        })
    met_final = False
    if last:
        met_final, ex = oracle_met(page, last)
        if met_final:
            extra = ex
    rec.update(extra)
    if met_final:
        verdict, reason = "пройдено", "оракул выполнен"
    elif not last:
        verdict, reason = "отказ", "страница не отвечает на зонд (навигация не удалась или окно умерло)"
    elif loads >= LOOP_LOADS:
        verdict, reason = "петля", f"{loads} загрузок документа за {elapsed} с"
    elif is_net_error(last):
        verdict, reason = "отказ", "страница сетевой ошибки браузера: " + last.get("title", "")[:60]
    elif is_denied(last):
        verdict, reason = "отказ", "страница показала отказ: " + last.get("title", "")[:60]
    elif is_challenge(last):
        verdict, reason = "завис", "страница-проверка не завершилась за окно наблюдения"
    elif page["oracle"] == "unblocked":
        verdict, reason = "завис", "итоговая страница не загрузилась (rs=%s, текст %s симв.)" % (last.get("rs"), last.get("text_len"))
    else:
        verdict, reason = "не_отрисован", {"token": "токена нет", "render": "iframe виджета нет", "score": "балла v3 нет"}[page["oracle"]]
    rec["verdict"], rec["reason"] = verdict, reason
    return rec


# ── плечо Lumen ──────────────────────────────────────────────────────────────

def run_lumen_page(exe: Path, page: dict, out_dir: Path, args) -> dict:
    br = perf_audit.LiveBrowser(exe, out_dir, args.nav_timeout, tag=page["id"])
    rec: dict = {}
    try:
        try:
            br.mcp.tool("navigate", {"url": page["url"]})
            try:
                br.mcp.tool("wait", {"condition": "document_ready", "timeout_ms": args.nav_timeout * 1000})
            except RuntimeError as e:  # не дождались готовности — наблюдаем всё равно, страница может грузиться долго
                rec["wait_error"] = str(e)[:160]
        except (RuntimeError, OSError) as e:
            rec.update({"verdict": "отказ", "reason": "навигация не удалась: " + str(e)[:160], "loads": 0})
            return rec

        def probe():
            try:
                return decode_probe(br.mcp.tool("eval", {"code": PROBE_JS}).get("result"))
            except (RuntimeError, OSError, ValueError):
                return None

        rec.update(observe(page, probe, args.observe_s, args.poll_s, args.settle_s))
        chunk = br.stderr_since(0)
        rec["stderr_errors"] = len(perf_audit.ERROR_RE.findall(chunk))
        rec["stderr_log"] = br.log_path.name
        return rec
    finally:
        br.close()


# ── плечо Chrome ─────────────────────────────────────────────────────────────

def kill_tree(proc: subprocess.Popen) -> None:
    """Chrome порождает рендереры: kill() родителя оставляет их жить и держать профиль."""
    subprocess.run(["taskkill", "/F", "/T", "/PID", str(proc.pid)], capture_output=True)
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        pass


def start_chrome(binary: str) -> tuple[subprocess.Popen, str, Cdp]:
    """Запуск со свежим профилем; до трёх повторов, если CDP-цель не поднялась (старт Chrome иногда не успевает)."""
    last: Exception | None = None
    for attempt in range(4):
        if attempt:
            time.sleep(3)
        port = free_port()
        profile = tempfile.mkdtemp(prefix="captcha-stand-chrome-")
        proc = subprocess.Popen(
            [binary, f"--remote-debugging-port={port}", f"--user-data-dir={profile}", "--no-first-run",
             "--no-default-browser-check", "about:blank"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        try:
            cdp = Cdp(WS(page_ws_url(port, timeout=20)))
            try:  # --start-maximized роняет Chrome 148 при старте (0xC0000005, воспроизводится), окно разворачиваем через CDP
                win = cdp.call("Browser.getWindowForTarget")
                cdp.call("Browser.setWindowBounds", {"windowId": win["windowId"], "bounds": {"windowState": "maximized"}})
            except (RuntimeError, TimeoutError, OSError, KeyError):
                pass
            try:  # окно за другими окнами душит таймеры страницы и ломает reCAPTCHA: держим вкладку «на переднем плане»
                cdp.call("Page.bringToFront")
                cdp.call("Emulation.setFocusEmulationEnabled", {"enabled": True})
            except (RuntimeError, TimeoutError, OSError):
                pass
            return proc, profile, cdp
        except (RuntimeError, OSError) as e:
            last = f"{e} (код выхода Chrome: {proc.poll()})"
            kill_tree(proc)
            shutil.rmtree(profile, ignore_errors=True)
    raise RuntimeError(f"Chrome не поднял CDP-цель за 4 попытки: {last}")


def run_chrome_page(binary: str, page: dict, out_dir: Path, args) -> dict:
    proc, profile, cdp = start_chrome(binary)
    rec: dict = {}
    try:
        cdp.call("Page.enable")
        try:
            r = cdp.call("Page.navigate", {"url": page["url"]}, timeout=args.nav_timeout)
            if r.get("errorText"):
                rec.update({"verdict": "отказ", "reason": "навигация не удалась: " + r["errorText"], "loads": 0})
                return rec
        except (RuntimeError, TimeoutError, OSError) as e:
            rec.update({"verdict": "отказ", "reason": "навигация не удалась: " + str(e)[:160], "loads": 0})
            return rec

        def probe():
            cdp.events.clear()
            try:
                return decode_probe(cdp.eval(PROBE_JS, timeout=15))
            except (RuntimeError, TimeoutError, OSError, ValueError):
                return None

        rec.update(observe(page, probe, args.observe_s, args.poll_s, args.settle_s))
        return rec
    finally:
        kill_tree(proc)
        shutil.rmtree(profile, ignore_errors=True)


# ── отчёт ────────────────────────────────────────────────────────────────────

def gpr(results: list[dict]) -> dict:
    n = len(results)
    ok = sum(1 for r in results if r.get("verdict") == "пройдено")
    by_vendor: dict[str, list[int]] = {}
    for r in results:
        v = by_vendor.setdefault(r["vendor"], [0, 0])
        v[1] += 1
        v[0] += r.get("verdict") == "пройдено"
    scores = [r["v3_score"] for r in results if "v3_score" in r]
    return {
        "pages": n, "passed": ok, "gpr": round(ok / n, 3) if n else None,
        "verdicts": {v: sum(1 for r in results if r.get("verdict") == v) for v in VERDICTS},
        "by_vendor": {k: f"{a}/{b}" for k, (a, b) in sorted(by_vendor.items())},
        "v3_scores": scores,
    }


def print_table(results: list[dict]) -> None:
    print(f"{'id':32} {'вендор':12} {'оракул':9} {'вердикт':13} загр. причина")
    for r in results:
        print(f"{r['id']:32} {r['vendor']:12} {r['oracle']:9} {r['verdict']:13} {r.get('loads', 0):4} {r.get('reason', '')[:70]}")


def compare_arms(cur: list[dict], other: dict) -> list[str]:
    other_by_id = {r["id"]: r for r in other["results"]}
    lines = []
    for r in cur:
        o = other_by_id.get(r["id"])
        if o and o["verdict"] != r["verdict"]:
            lines.append(f"{r['id']}: {r['verdict']} (здесь) vs {o['verdict']} ({other.get('arm')})")
    return lines


def regressions(cur: list[dict], base: dict) -> list[str]:
    base_by_id = {r["id"]: r for r in base["results"]}
    return [
        f"{r['id']}: было пройдено, стало {r['verdict']} ({r.get('reason', '')[:60]})"
        for r in cur
        if base_by_id.get(r["id"], {}).get("verdict") == "пройдено" and r["verdict"] != "пройдено"
    ]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--arm", choices=("lumen", "chrome"), required=True)
    ap.add_argument("--corpus", type=Path, default=DEFAULT_CORPUS)
    ap.add_argument("--only", default="", help="id страниц через запятую")
    ap.add_argument("--exe", help="lumen.exe (плечо lumen; иначе $LUMEN_EXE / target/)")
    ap.add_argument("--binary", help="chrome.exe (плечо chrome; иначе Chromium из кэша Playwright)")
    ap.add_argument("--env", action="append", default=[], help="KEY=VAL в окружение lumen (повторяемый), напр. ручка идентичности")
    ap.add_argument("--label", default="", help="метка варианта в отчёте (напр. identity=chrome_lumen)")
    ap.add_argument("--route", default="", help="метка сетевого маршрута (напрямую / VPN-страна)")
    ap.add_argument("--observe-s", type=float, default=60.0, help="окно наблюдения на страницу, с (60 = минута для критерия петли)")
    ap.add_argument("--poll-s", type=float, default=2.0)
    ap.add_argument("--settle-s", type=float, default=6.0, help="сколько оракул должен держаться до раннего выхода")
    ap.add_argument("--nav-timeout", type=int, default=45, help="ожидание document_ready/навигации, с")
    ap.add_argument("--out", type=Path)
    ap.add_argument("--baseline", type=Path)
    ap.add_argument("--compare", type=Path)
    args = ap.parse_args()

    for kv in args.env:
        k, _, v = kv.partition("=")
        perf_audit.EXTRA_ENV[k] = v
    pages = load_corpus(args.corpus, {s for s in args.only.split(",") if s})
    stamp = datetime.now().strftime("%Y-%m-%d-%H%M")
    out_dir = OUT_ROOT / f"{stamp}-{args.arm}"
    out_dir.mkdir(parents=True, exist_ok=True)
    out_json = args.out or (OUT_ROOT / f"{stamp}-{args.arm}.json")

    if args.arm == "lumen":
        exe = perf_audit.find_exe(args.exe)
        build = {"exe": str(exe), "exe_mtime": datetime.fromtimestamp(exe.stat().st_mtime).isoformat(timespec="minutes")}
        runner = lambda pg: run_lumen_page(exe, pg, out_dir, args)  # noqa: E731
    else:
        binary = args.binary or default_binary()
        if not binary:
            sys.exit("Chromium не найден: --binary или кэш Playwright")
        build = {"binary": binary, "chrome_version": chromium_version(binary)}
        runner = lambda pg: run_chrome_page(binary, pg, out_dir, args)  # noqa: E731

    results = []
    for i, page in enumerate(pages, 1):
        t0 = time.monotonic()
        try:
            rec = runner(page)
        except Exception as e:  # noqa: BLE001  один сбой страницы не должен снимать весь прогон
            rec = {"verdict": "отказ", "reason": f"сбой стенда: {type(e).__name__}: {str(e)[:140]}", "loads": 0}
        rec.update({k: page[k] for k in ("id", "vendor", "oracle", "url")})
        rec["wall_s"] = round(time.monotonic() - t0, 1)
        results.append(rec)
        print(f"[{i}/{len(pages)}] {page['id']}: {rec['verdict']} — {rec.get('reason', '')[:80]}", flush=True)
        out_json.parent.mkdir(parents=True, exist_ok=True)
        out_json.write_text(json.dumps({"arm": args.arm, "label": args.label, "route": args.route, "build": build,
                                        "env": perf_audit.EXTRA_ENV, "started": stamp, "summary": gpr(results),
                                        "results": results}, ensure_ascii=False, indent=1), encoding="utf-8")

    print()
    print_table(results)
    s = gpr(results)
    print(f"\nGPR = {s['passed']}/{s['pages']} = {s['gpr']}; вердикты: {s['verdicts']}; по вендорам: {s['by_vendor']}; балл v3: {s['v3_scores'] or '—'}")
    print(f"JSON: {out_json}")
    rc = 0
    if args.compare:
        diff = compare_arms(results, json.loads(args.compare.read_text(encoding="utf-8")))
        print("\nРасхождения с %s:" % args.compare.name, *(diff or ["нет"]), sep="\n  ")
    if args.baseline:
        reg = regressions(results, json.loads(args.baseline.read_text(encoding="utf-8")))
        print("\nРегрессии к %s:" % args.baseline.name, *(reg or ["нет"]), sep="\n  ")
        rc = 1 if reg else 0
    return rc


if __name__ == "__main__":
    sys.exit(main())
