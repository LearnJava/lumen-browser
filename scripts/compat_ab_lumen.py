#!/usr/bin/env python3
"""Плечо «стек Lumen» A/B-прогона идентичности (COMPAT-AB-IDENTITY).

Парное к `compat_ab_chrome.py`: тот ходит настоящим Chrome for Testing, этот — самим
`lumen --dump-source <url>` (сетевой стек Lumen целиком: DNS, rustls, H1/H2, набор
заголовков и UA из `LUMEN_IDENTITY`, редиректы, декодирование). Варианты те же:

  * `plain` — `LUMEN_IDENTITY=chrome_unbranded` (стоковая грамматика Chrome);
  * `lumen` — `LUMEN_IDENTITY=chrome_lumen` (+ ` Lumen/x.y` в UA, бренд `Lumen` в Sec-CH-UA).

Почему не живое окно (`perf_audit.py`, `compat_crawl.py`): оно не отдаёт первый кадр до конца
`wait` (BUG-1629), все сайты выходят `HUNG`. `--dump-source` запрашивает только главный документ:
статус и тело ответа есть, JS не исполняется. Значит, стены, которые сайт собирает скриптом после
загрузки (проверка браузера Cloudflare, «Unsupported browser» из JS), здесь не видны, а в плече
Chrome видны — сравнивая стек Lumen и Chrome, смотреть надо на HTTP-слой (`http_status`, стена по
телу ответа), а не на итоговый `status`. Внутри одного плеча `plain` против `lumen` честно
сравнимы всегда. Профиль на каждый запуск свежий (cookie-банка в памяти), процесс на сайт.

    python scripts/compat_ab_lumen.py --exe target/dev-release/lumen.exe --corpus docs/perf/corpus-ab.txt
    python scripts/compat_ab_lumen.py --url zillow=https://www.zillow.com/ --variant lumen

Результат: `<out>/results.json` (та же схема записей, что у `compat_ab_chrome.py`:
`slug`/`url`/`variant`/`status`/`http_status`/`wall`/…), `<out>/summary.md`. `--json PATH` копирует
results.json в `docs/perf/runs/`. Свести четыре ячейки — `scripts/compat_ab_matrix.py`.
Нужен Python ≥ 3.10, только stdlib.
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
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from compat_ab_chrome import BLOCKED, load_corpus, lumen_chrome_major, lumen_version  # noqa: E402
from wall_signatures import classify_wall  # noqa: E402

REPO = Path(__file__).resolve().parent.parent

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

VARIANTS = ("plain", "lumen")
IDENTITY = {"plain": "chrome_unbranded", "lumen": "chrome_lumen"}
MAX_BODY = 400_000
PROFILE_ROOT = REPO / ".tmp" / "ab-lumen-profiles"
# stderr у `--dump-source`: «→ GET <url>» на каждый запрос (редиректы — отдельные), «← <код> <url>» на каждый ответ,
# «✗ <url> (<класс>: <текст>)» при сетевом сбое.
RE_RESP = re.compile(r"^← (\d{3}) (\S+)", re.M)
RE_FAIL = re.compile(r"^✗ \S+ \((\w+):\s*(.*)\)\s*$", re.M)


def find_lumen(explicit: str | None) -> Path:
    for cand in (explicit, os.environ.get("LUMEN_EXE")):
        if cand and Path(cand).is_file():
            return Path(cand)
    for rel in ("target/dev-release/lumen.exe", "target/release/lumen.exe", "target/debug/lumen.exe"):
        if (REPO / rel).is_file():
            return REPO / rel
    sys.exit("lumen.exe не найден: --exe или $LUMEN_EXE")


def parse_stderr(err: str) -> dict:
    """Итог запроса из stderr `--dump-source`: последний ответ, число редиректов, сетевой сбой."""
    resp = RE_RESP.findall(err)
    fail = RE_FAIL.search(err)
    return {
        "http_status": int(resp[-1][0]) if resp else None,
        "final_url": resp[-1][1] if resp else None,
        "redirects": max(len(resp) - 1, 0),
        "net_error": f"{fail.group(1)}: {fail.group(2)}" if fail else None,
    }


RE_HIDDEN = re.compile(r"<!--.*?-->|<(script|style|noscript|template|svg)\b.*?</\1\s*>", re.S | re.I)
RE_TAG = re.compile(r"<[^>]*>")


def visible_text(html: str) -> str:
    """Видимый текст исходника: без скриптов, стилей, `<noscript>` и комментариев. Фразы «unsupported browser»
    лежат в бандлах и JSON больших страниц (tiktok, t-online) — по сырому исходнику это ложная стена; Chrome-плечо
    ищет по `innerText`, то есть без них."""
    return RE_TAG.sub(" ", RE_HIDDEN.sub(" ", html))


def classify(rec: dict, wall: dict) -> str:
    """Таксономия `compat_ab_chrome.classify` без DOM-признаков: JS здесь не исполняется."""
    st = rec.get("http_status")
    if st is None:
        return "NET_FAIL" if rec.get("net_error") else "TIMEOUT"
    if wall["kind"] in ("hard", "refused") or st in (401, 403, 429):
        return "SITE_REFUSED"
    if wall["kind"] == "soft":
        return "WALL_SOFT"
    if st >= 400:
        return "HTTP_ERROR"
    return "OK" if rec.get("body_bytes") else "EMPTY"


def remove_profile(path: Path) -> None:
    """Каталог профиля: сразу после выхода процесса Windows ещё держит exe (антивирус, индексатор) — повторы."""
    for _ in range(40):
        shutil.rmtree(path, ignore_errors=True)
        if not path.exists():
            return
        time.sleep(0.5)


def audit_site(exe: Path, slug: str, url: str, variant: str, timeout: float, extra_env: dict[str, str]) -> dict:
    rec: dict = {"slug": slug, "url": url, "variant": variant, "arm": "lumen", "identity": IDENTITY[variant]}
    # Свежий профиль на запуск. Данные Lumen (дисковый HTTP-кэш, cookie, хранилища) лежат в `<каталог exe>/data`
    # (`adblock::browser_data_dir`), и без этого второй вариант получил бы ответ из кэша первого: в stderr нет ни
    # «→ GET», ни «← код». Жёсткая ссылка на exe в новом каталоге даёт чистый `data/` без копирования 100 МБ.
    profile = Path(tempfile.mkdtemp(prefix="ab-lumen-", dir=PROFILE_ROOT))
    local_exe = profile / "lumen.exe"
    try:
        os.link(exe, local_exe)
    except OSError:  # другой том
        shutil.copyfile(exe, local_exe)
    env = dict(os.environ, LUMEN_NO_ADBLOCK="1", LUMEN_IDENTITY=IDENTITY[variant], **extra_env)
    t0 = time.monotonic()
    body = ""
    try:
        p = subprocess.run([str(local_exe), "--dump-source", url], capture_output=True, env=env, timeout=timeout)
        err = p.stderr.decode("utf-8", errors="replace")
        body = p.stdout.decode("utf-8", errors="replace")
        rec.update(parse_stderr(err))
        rec["exit_code"] = p.returncode
        if p.returncode != 0 and not rec["net_error"]:
            rec["net_error"] = f"exit {p.returncode}: " + (err.strip().splitlines() or [""])[-1][:200]
    except subprocess.TimeoutExpired:
        rec.update(http_status=None, final_url=None, redirects=0, net_error=None, exit_code=None)
    finally:
        remove_profile(profile)
    rec["body_bytes"] = len(body.encode("utf-8"))
    wall = classify_wall(rec.get("http_status"), None, body[:MAX_BODY], net_error=rec.get("net_error"), visible_text=visible_text(body[:MAX_BODY]))
    rec["wall"] = wall
    rec["status"] = classify(rec, wall)
    rec["wall_s"] = round(time.monotonic() - t0, 1)
    return rec


def summary_md(results: list[dict], variants: list[str], meta: dict) -> str:
    statuses = sorted({r["status"] for r in results})
    out = [
        "# Плечо «стек Lumen»: A/B идентичности (`--dump-source`, HTTP-слой)",
        "",
        f"Lumen {meta['lumen_version']} (Chrome {meta['lumen_major']}), {meta['date']}. JS не исполняется (BUG-1629 закрывает живое окно).",
        "",
        "| Вариант | " + " | ".join(statuses) + " |",
        "|---|" + "---|" * len(statuses),
    ]
    for v in variants:
        rs = [r for r in results if r["variant"] == v]
        out.append(f"| {v} ({len(rs)}) | " + " | ".join(str(sum(1 for r in rs if r["status"] == s)) for s in statuses) + " |")
    slugs = list(dict.fromkeys(r["slug"] for r in results))
    diff = [s for s in slugs if len({r["status"] in BLOCKED for r in results if r["slug"] == s}) > 1]
    if len(variants) > 1:
        out += ["", f"Сайтов, где блокировка зависит от токена: {len(diff)} из {len(slugs)}" + (f" — {', '.join(diff)}" if diff else "")
                + f". Блокировка = {', '.join(sorted(BLOCKED))}."]
    return "\n".join(out) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--exe", help="lumen.exe (иначе $LUMEN_EXE / target/dev-release)")
    ap.add_argument("--corpus", type=Path, help="файл `slug url`")
    ap.add_argument("--url", action="append", default=[], metavar="SLUG=URL", help="один сайт; можно несколько раз")
    ap.add_argument("--only", action="append", default=[], help="фильтр корпуса по подстроке slug")
    ap.add_argument("--variant", action="append", choices=VARIANTS, help="вариант идентичности (по умолчанию оба)")
    ap.add_argument("--timeout", type=float, default=60, help="секунд на запуск процесса")
    ap.add_argument("--jobs", type=int, default=4, help="одновременных процессов lumen")
    ap.add_argument("--env", action="append", default=[], metavar="KEY=VAL", help="доп. окружение для lumen")
    ap.add_argument("--out-dir", type=Path)
    ap.add_argument("--json", type=Path, help="скопировать results.json сюда (docs/perf/runs/<дата>-ab-lumen.json)")
    args = ap.parse_args()

    sites = [tuple(u.split("=", 1)) for u in args.url]
    if args.corpus:
        sites += load_corpus(args.corpus, args.only)
    if not sites:
        ap.error("нужен --corpus или --url")
    variants = args.variant or list(VARIANTS)
    exe = find_lumen(args.exe)
    extra_env = dict(e.split("=", 1) for e in args.env)
    out_dir = args.out_dir or REPO / ".tmp" / "compat-ab-lumen" / datetime.now().strftime("%Y%m%d-%H%M%S")
    out_dir.mkdir(parents=True, exist_ok=True)
    PROFILE_ROOT.mkdir(parents=True, exist_ok=True)
    meta = {
        "arm": "lumen", "exe": str(exe), "lumen_major": lumen_chrome_major(), "lumen_version": lumen_version(),
        "date": datetime.now().strftime("%Y-%m-%d"), "mode": "dump-source", "env": extra_env,
    }
    jobs = [(slug, url, v) for slug, url in sites for v in variants]
    results: list[dict] = []
    with ThreadPoolExecutor(max_workers=max(args.jobs, 1)) as pool:
        # pool.map держит порядок корпуса; печать идёт по мере готовности соседних записей.
        for rec in pool.map(lambda j: audit_site(exe, j[0], j[1], j[2], args.timeout, extra_env), jobs):
            results.append(rec)
            w = rec.get("wall") or {}
            print(f"{rec['slug']:<26} {rec['variant']:<6} {rec['status']:<13} http={rec.get('http_status')} "
                  f"wall={w.get('kind')}/{w.get('vendor')} {rec['wall_s']}s", flush=True)
            (out_dir / "results.json").write_text(json.dumps({"meta": meta, "results": results}, ensure_ascii=False, indent=1), encoding="utf-8")
    for left in PROFILE_ROOT.iterdir():  # профили, которые Windows не отпустил вовремя
        remove_profile(left)
    (out_dir / "summary.md").write_text(summary_md(results, variants, meta), encoding="utf-8")
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(out_dir / "results.json", args.json)
    print(f"\n{out_dir / 'summary.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
