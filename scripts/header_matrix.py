#!/usr/bin/env python3
"""Матрица «TLS-отпечаток × набор заголовков» и бисект по одному заголовку
(COMPAT-AB-IDENTITY-HARNESS; воспроизводит методику BUG-1113, скрипты которой
пропали вместе с `.tmp`).

Вопрос, на который отвечает: сайт отказал из-за *TLS-отпечатка* или из-за
*заголовков*? Две оси независимы:

  * стек TLS: `chrome_tls` — `curl_cffi` с `impersonate="chrome"` (отпечаток и
    H2-настройки браузера); `plain_tls` — `httpx` h2 поверх OpenSSL
    (не-браузерный отпечаток, как у rustls);
  * набор заголовков — JSON-файл в `docs/perf/header-sets/`: либо снят с провода
    (`capture`: настоящий Chrome и Lumen против `header_echo_server.py`), либо
    исторический (`lumen-2026-09-23` — до фикса BUG-1113, из `headers.rs@4afbae977^`).

Подкоманды:
    capture --lumen-exe lumen.exe        снять наборы Chrome (plain, lumen) и Lumen (обе идентичности)
    matrix  --sets A B [--stacks ...]    сайты × (стек × набор)
    bisect  --base A --target B          от набора A менять по одному заголовку на значение из B
    wire    --sets A B                   что библиотеки реально шлют (эхо-сервер вместо сайта)
    list                                 какие наборы есть

Примеры:
    python scripts/header_matrix.py capture --lumen-exe target/dev-release/lumen.exe
    python scripts/header_matrix.py matrix --corpus docs/perf/ab-bug1113.txt --sets lumen-2026-09-23 chrome-153-plain
    python scripts/header_matrix.py bisect --corpus docs/perf/ab-bug1113.txt --base lumen-2026-09-23 --target chrome-153-plain

Зависимости (нет в стандартной установке): `pip install curl_cffi "httpx[http2]"`.
Библиотеки могут дописать или переставить заголовки; что ушло на провод, показывает `wire`.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from wall_signatures import classify_wall  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
SETS_DIR = REPO / "docs" / "perf" / "header-sets"
STACKS = ("chrome_tls", "plain_tls")
# Эти заголовки ставит HTTP-библиотека сама (соединение, длина, адрес).
LIBRARY_OWNED = {"host", "connection", "content-length", "transfer-encoding"}
# curl на plain HTTP предлагает h2c-апгрейд; по TLS этих заголовков нет.
H1_UPGRADE_NOISE = {"upgrade", "http2-settings"}

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")


# ── наборы заголовков ────────────────────────────────────────────────────────

def load_set(name: str) -> dict:
    p = SETS_DIR / f"{name}.json"
    if not p.is_file():
        sys.exit(f"нет набора {name!r} ({p}); есть: {', '.join(sorted(s.stem for s in SETS_DIR.glob('*.json')))}")
    return json.loads(p.read_text(encoding="utf-8"))


def wire_headers(hset: dict) -> list[tuple[str, str]]:
    """Заголовки набора без тех, что ставит библиотека, в исходном порядке."""
    return [(k, v) for k, v in hset["headers"] if k.lower() not in LIBRARY_OWNED]


def save_set(name: str, source: str, headers: list[list[str]], note: str = "") -> Path:
    SETS_DIR.mkdir(parents=True, exist_ok=True)
    p = SETS_DIR / f"{name}.json"
    data = {"name": name, "source": source, "note": note, "captured": datetime.now().strftime("%Y-%m-%d"), "headers": headers}
    p.write_text(json.dumps(data, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    return p


# ── стеки TLS ────────────────────────────────────────────────────────────────

def classify_error(exc: Exception) -> str:
    msg = f"{type(exc).__name__}: {exc}".lower()
    if "timed out" in msg or "timeout" in msg:
        return "TIMEOUT"
    if "stream" in msg and ("reset" in msg or "not closed cleanly" in msg or "rst" in msg or "error code" in msg or "92" in msg):
        return "RST_STREAM"
    if "connectionterminated" in msg or "goaway" in msg:
        return "GOAWAY"
    return f"ERR:{type(exc).__name__}"


def fetch(stack: str, url: str, headers: list[tuple[str, str]], timeout: float) -> dict:
    """Один GET. Возвращает `{status, wall, error, protocol, bytes}`."""
    try:
        if stack == "chrome_tls":
            from curl_cffi import requests as cr  # noqa: PLC0415

            r = cr.get(url, headers=dict(headers), impersonate="chrome", default_headers=False,
                       timeout=timeout, allow_redirects=True, max_redirects=5)
            status, body, hdrs, proto = r.status_code, r.text, dict(r.headers), str(getattr(r, "http_version", ""))
        elif stack == "plain_tls":
            import httpx  # noqa: PLC0415

            with httpx.Client(http2=True, timeout=timeout, follow_redirects=True, max_redirects=5, headers=dict(headers)) as c:
                r = c.get(url)
            status, body, hdrs, proto = r.status_code, r.text, dict(r.headers), r.http_version
        else:
            raise ValueError(stack)
    except ImportError as exc:
        sys.exit(f"{exc}: pip install curl_cffi \"httpx[http2]\"")
    except Exception as exc:  # noqa: BLE001 — любой сбой сети это исход ячейки
        return {"status": None, "error": classify_error(exc), "detail": str(exc)[:200]}
    wall = classify_wall(status, hdrs, body)
    return {"status": status, "wall": wall, "bytes": len(body), "protocol": proto, "error": None}


def cell(stack: str, url: str, headers: list[tuple[str, str]], repeat: int, timeout: float) -> dict:
    runs = [fetch(stack, url, headers, timeout) for _ in range(repeat)]
    labels = []
    for r in runs:
        if r["error"]:
            labels.append(r["error"])
        else:
            w = r["wall"]
            tag = f" {w['vendor']}" if w["kind"] in ("hard", "soft") and w.get("vendor") else ""
            labels.append(f"{r['status']}{tag}")
    return {"label": " / ".join(labels), "runs": runs, "ok": all(not r["error"] and r["status"] == 200 and r["wall"]["kind"] == "none" for r in runs)}


# ── команды ──────────────────────────────────────────────────────────────────

def read_sites(args) -> list[tuple[str, str]]:
    sites = [tuple(u.split("=", 1)) for u in args.url]
    if args.corpus:
        for line in args.corpus.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                slug, url = line.split(None, 1)
                sites.append((slug, url.strip()))
    if not sites:
        sys.exit("нужен --corpus или --url SLUG=URL")
    return sites


def md_table(head: list[str], rows: list[list[str]]) -> str:
    out = ["| " + " | ".join(head) + " |", "|" + "---|" * len(head)]
    out += ["| " + " | ".join(r) + " |" for r in rows]
    return "\n".join(out)


def cmd_matrix(args) -> int:
    sites = read_sites(args)
    sets = {n: load_set(n) for n in args.sets}
    cols = [(st, n) for st in args.stacks for n in sets]
    rows, raw = [], {}
    for slug, url in sites:
        row = [slug]
        for st, n in cols:
            c = cell(st, url, wire_headers(sets[n]), args.repeat, args.timeout)
            raw[f"{slug}|{st}|{n}"] = c
            row.append(c["label"])
            print(f"{slug:<18} {st:<10} {n:<22} {c['label']}", flush=True)
        rows.append(row)
    table = md_table(["Сайт"] + [f"{st} + {n}" for st, n in cols], rows)
    emit(args, "Матрица TLS × заголовки", table, raw)
    return 0


def variants_from(base: dict, target: dict) -> list[tuple[str, list[tuple[str, str]]]]:
    """Набор A, у которого по одному заголовку заменено значением из B (нет в B — убран, нет в A — добавлен)."""
    b, t = wire_headers(base), wire_headers(target)
    bm = {k.lower(): (k, v) for k, v in b}
    tm = {k.lower(): (k, v) for k, v in t}
    out: list[tuple[str, list[tuple[str, str]]]] = [("как есть", b)]
    for low in list(bm) + [k for k in tm if k not in bm]:
        if bm.get(low, (None, None))[1] == tm.get(low, (None, None))[1]:
            continue
        if low in bm and low in tm:
            name, hs = f"{bm[low][0]} → B", [(k, tm[low][1] if k.lower() == low else v) for k, v in b]
        elif low in bm:
            name, hs = f"без {bm[low][0]}", [(k, v) for k, v in b if k.lower() != low]
        else:
            name, hs = f"+ {tm[low][0]}", b + [tm[low]]
        out.append((name, hs))
    out.append(("весь набор B", t))
    return out


def cmd_bisect(args) -> int:
    sites = read_sites(args)
    base, target = load_set(args.base), load_set(args.target)
    variants = variants_from(base, target)
    print(f"вариантов: {len(variants)} (A={args.base}, B={args.target}, стек {args.stack})", flush=True)
    rows, raw = [], {}
    for slug, url in sites:
        row = [slug]
        for name, hs in variants:
            c = cell(args.stack, url, hs, args.repeat, args.timeout)
            raw[f"{slug}|{name}"] = c
            row.append(c["label"])
        print(f"{slug:<18} " + " | ".join(row[1:]), flush=True)
        rows.append(row)
    table = md_table(["Сайт"] + [n for n, _ in variants], rows)
    emit(args, f"Бисект от {args.base} к {args.target} ({args.stack})", table, raw)
    return 0


def emit(args, title: str, table: str, raw: dict) -> None:
    md = f"## {title}\n\nДата: {datetime.now():%Y-%m-%d}. Повторов на ячейку: {args.repeat}.\n\n{table}\n"
    print("\n" + md)
    if args.md:
        args.md.parent.mkdir(parents=True, exist_ok=True)
        args.md.write_text(md, encoding="utf-8")
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(raw, ensure_ascii=False, indent=1), encoding="utf-8")


def cmd_list(_args) -> int:
    for p in sorted(SETS_DIR.glob("*.json")):
        d = json.loads(p.read_text(encoding="utf-8"))
        print(f"{d['name']:<28} {len(d['headers']):>2} заголовков  {d['source']}")
    return 0


def cmd_wire(args) -> int:
    """Сверить набор с тем, что библиотека отправила на эхо-сервер (plain HTTP, h1)."""
    from header_echo_server import EchoServer  # noqa: PLC0415

    bad = 0
    for st in args.stacks:
        for n in args.sets:
            want = wire_headers(load_set(n))
            with EchoServer() as echo:
                fetch(st, echo.url + "/", want, 10)
                got = echo.navigations()
            if not got:
                print(f"{st} + {n}: запрос не дошёл")
                bad += 1
                continue
            sent = {k.lower(): v for k, v in got[0]["headers"]}
            wanted = {k.lower(): v for k, v in want}
            extra = sorted(k for k in sent if k not in wanted and k not in LIBRARY_OWNED | H1_UPGRADE_NOISE)
            missing = sorted(k for k in wanted if k not in sent)
            changed = sorted(k for k in wanted if k in sent and sent[k] != wanted[k])
            ok = not (extra or missing or changed)
            bad += not ok
            print(f"{st} + {n}: {'совпало' if ok else 'РАСХОЖДЕНИЕ'}"
                  + (f"; лишние {extra}" if extra else "") + (f"; потеряны {missing}" if missing else "")
                  + (f"; изменены {changed}" if changed else ""))
    return 1 if bad else 0


def cmd_capture(args) -> int:
    """Снять наборы с провода: Chrome (оба варианта) и, если дан бинарник, Lumen (обе идентичности)."""
    from compat_ab_chrome import ChromeSession, find_chrome, lumen_chrome_major, lumen_version  # noqa: PLC0415
    from header_echo_server import EchoServer  # noqa: PLC0415

    major, lumen_ver = lumen_chrome_major(), lumen_version()
    binary, chrome_ver = find_chrome(args.binary, major)
    for variant in ("plain", "lumen"):
        with EchoServer() as echo:
            sess = ChromeSession(binary, variant, major, lumen_ver, visible=False, ua_major=None)
            try:
                sess.setup()
                sess.page.call("Page.navigate", {"url": echo.url + "/"})
                sess.page.pump(15, until=lambda: sess.page.loaded)
            finally:
                sess.close()
            nav = echo.navigations()
            if not nav:
                sys.exit(f"Chrome {variant}: эхо-сервер не получил запрос")
            p = save_set(f"chrome-{major}-{variant}", f"Chrome for Testing {chrome_ver}, headless=new, UA-вариант {variant}",
                         nav[0]["headers"], "plain HTTP на 127.0.0.1; по HTTPS добавляются Sec-Fetch-* для не-localhost и HSTS-логика")
            print("записан", p)
    if args.lumen_exe:
        for ident in ("chrome_unbranded", "chrome_lumen"):
            with EchoServer() as echo:
                env = dict(os.environ, LUMEN_IDENTITY=ident, LUMEN_NO_ADBLOCK="1")
                subprocess.run([args.lumen_exe, "--dump-source", echo.url + "/"], env=env, capture_output=True, timeout=120)
                nav = echo.navigations()
                if not nav:
                    sys.exit(f"Lumen {ident}: эхо-сервер не получил запрос")
                p = save_set(f"lumen-{ident}", f"{args.lumen_exe} --dump-source, LUMEN_IDENTITY={ident}", nav[0]["headers"],
                             "plain HTTP на 127.0.0.1")
                print("записан", p)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)

    def common(p, stack_multi: bool) -> None:
        p.add_argument("--corpus", type=Path, help="файл `slug url`")
        p.add_argument("--url", action="append", default=[], metavar="SLUG=URL")
        p.add_argument("--repeat", type=int, default=2, help="повторов на ячейку (BUG-1113: 2)")
        p.add_argument("--timeout", type=float, default=30)
        p.add_argument("--md", type=Path, help="записать таблицу сюда")
        p.add_argument("--json", type=Path, help="записать сырые ответы сюда")

    m = sub.add_parser("matrix", help="сайты × (стек × набор)")
    common(m, True)
    m.add_argument("--sets", nargs="+", required=True)
    m.add_argument("--stacks", nargs="+", choices=STACKS, default=list(STACKS))
    m.set_defaults(fn=cmd_matrix)

    b = sub.add_parser("bisect", help="от набора A менять по одному заголовку на значение из B")
    common(b, False)
    b.add_argument("--base", required=True)
    b.add_argument("--target", required=True)
    b.add_argument("--stack", choices=STACKS, default="plain_tls", help="BUG-1113 бисектил на plain_tls")
    b.set_defaults(fn=cmd_bisect)

    c = sub.add_parser("capture", help="снять наборы с эхо-сервера")
    c.add_argument("--binary", help="chrome.exe (по умолчанию CfT из .tmp/cft)")
    c.add_argument("--lumen-exe", help="lumen.exe; без него снимается только Chrome")
    c.set_defaults(fn=cmd_capture)

    w = sub.add_parser("wire", help="сверить набор с отправленным на эхо-сервер")
    w.add_argument("--sets", nargs="+", required=True)
    w.add_argument("--stacks", nargs="+", choices=STACKS, default=list(STACKS))
    w.set_defaults(fn=cmd_wire)

    sub.add_parser("list", help="какие наборы есть").set_defaults(fn=cmd_list)

    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
