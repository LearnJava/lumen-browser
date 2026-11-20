#!/usr/bin/env python3
"""Self-check единого Chrome-фингерпринта (FINGERPRINT-MANIFEST, §5.3).

Тянет `https://tls.peet.ws/api/all` двумя браузерами с одной машины и
свежими профилями:

  * `lumen` — `lumen.exe --mcp-live-port`, eval через MCP JSON-RPC
    (тот же транспорт, что у `probe_native_api.py`);
  * `chrome` — Chrome for Testing 157 по CDP без UA-override
    (настоящий Chrome-фингерпринт как эталон).

Сырые JSON складываются в `--out-dir/<date>-peet-{lumen,chrome}.json`,
в stdout — сравнение: SETTINGS-фреймы H2 (значения и относительный порядок),
`akamai_fingerprint`, порядок заголовков, `tls.ja3`/`ja4` (хэши записываются,
но не гейтят: session state их может сдвинуть).

Если peet недоступен с маршрута — fallback `https://tls.browserleaks.com/json`
(JA3/JA4) + локальный `header_echo_server` для порядка заголовков; подмена
фиксируется в записи прогона (`source` в JSON и строка в stdout).

    python scripts/fingerprint_selfcheck.py --out-dir docs/perf/runs --date 2026-11-20

Нужен Python ≥ 3.10, только stdlib.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request
from pathlib import Path

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from compat_ab_chrome import find_chrome, lumen_chrome_major  # noqa: E402
from header_echo_server import EchoServer  # noqa: E402
from probe_native_api import ChromeSession, LumenSession  # noqa: E402

REPO = Path(__file__).resolve().parent.parent

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

PEET_URL = "https://tls.peet.ws/api/all"
BLEED_URL = "https://tls.browserleaks.com/json"


KICK_JS = (
    "window.__peet_body=null;"
    "fetch(location.href).then(r=>r.text())"
    ".then(t=>{window.__peet_body=t})"
    ".catch(e=>{window.__peet_body='ERR:'+String(e)}); 'kicked'"
)
POLL_JS = "typeof window.__peet_body==='string' ? window.__peet_body : null"
INNER_TEXT_JS = (
    "(document.body && document.body.innerText) || document.documentElement.innerText || ''"
)


def _looks_complete(text) -> bool:
    if not isinstance(text, str):
        return False
    s = text.strip()
    if not s.startswith("{"):
        return False
    try:
        json.loads(s)
        return True
    except Exception:
        return False


def fetch_body(session, url: str, timeout_s: float = 90.0) -> str:
    """Полное тело JSON-ответа `url`: сессия уже стоит на этой странице.

    MCP eval не ждёт promise, поэтому тело забираем в два шага: kick
    `fetch(location.href)` (same-origin subresource → h2, в отличие от
    document-навигации, которую PERF-STREAM-H1 держит на HTTP/1.1), затем
    poll `window.__peet_body`. Документный `innerText` — fallback: он
    отражает h1-навигацию и годится только когда fetch недоступен.
    """
    import time

    try:
        session.eval(KICK_JS)
    except Exception:
        pass
    deadline = time.monotonic() + timeout_s
    last_err = None
    # Фаза 1: только fetch-результат (subresource → h2). innerText здесь
    # не принимаем — это ответ document-навигации (h1, PERF-STREAM-H1) и
    # всегда «полный JSON», поэтому выигрывал бы гонку у pending fetch.
    fetch_deadline = time.monotonic() + min(45.0, timeout_s)
    while time.monotonic() < fetch_deadline:
        try:
            text = session.eval(POLL_JS)
            if _looks_complete(text):
                return text
            if isinstance(text, str) and text.startswith("ERR:"):
                last_err = text
        except Exception as ex:
            last_err = str(ex)
        time.sleep(1.0)
    # Фаза 2: fetch не дал результата — отдаём document body как есть
    # (фиксируем http_version=HTTP/1.1 в артефакте, это честный fallback).
    while time.monotonic() < deadline:
        try:
            text = session.eval(INNER_TEXT_JS)
            if _looks_complete(text):
                return text
        except Exception as ex:
            last_err = str(ex)
        time.sleep(1.0)
    raise RuntimeError(f"read {url} failed: {last_err}")


def reachable(url: str, timeout: float = 10.0) -> bool:
    try:
        urllib.request.urlopen(url, timeout=timeout).read(64)
        return True
    except Exception:
        return False


def extract_peet(raw: dict) -> dict:
    """То, что self-check сравнивает: H2 SETTINGS, akamai fp, порядок
    заголовков, JA3/JA4. Схема peet плавает — берём защитно."""
    out = {"source": "tls.peet.ws/api/all"}
    h2 = raw.get("http2") or {}
    frames = h2.get("sent_frames") or []
    settings = []
    for fr in frames:
        if (
            isinstance(fr, dict)
            and fr.get("frame_type") == "SETTINGS"
            and fr.get("settings") is not None
        ):
            settings.append(fr.get("settings"))
    out["h2_settings"] = settings
    out["akamai_fingerprint"] = h2.get("akamai_fingerprint")
    out["akamai_fingerprint_hash"] = h2.get("akamai_fingerprint_hash")
    headers = raw.get("headers") or {}
    out["header_order"] = headers.get("order") or headers.get("headers")
    tls = raw.get("tls") or {}
    out["tls"] = {
        k: tls.get(k)
        for k in ("ja3", "ja3_hash", "ja4", "peetprint", "peetprint_hash")
        if tls.get(k) is not None
    }
    out["http_version"] = raw.get("http_version")
    return out


def extract_bleed(raw: dict) -> dict:
    """Fallback-источник: browserleaks отдаёт JA3/JA4 без H2-деталей."""
    return {
        "source": "tls.browserleaks.com/json",
        "tls": {k: raw.get(k) for k in ("ja3", "ja3_hash", "ja4", "user_agent") if raw.get(k)},
        "http_version": raw.get("http_version"),
    }


def compare(lumen: dict, chrome: dict) -> list[str]:
    lines = []
    ls, cs = lumen.get("h2_settings"), chrome.get("h2_settings")
    if ls is not None or cs is not None:
        if ls == cs:
            lines.append(f"h2 SETTINGS: equal ({len(ls or [])} frames)")
        else:
            lines.append(f"h2 SETTINGS: DIFFER\n  lumen : {ls}\n  chrome: {cs}")
    la, ca = lumen.get("akamai_fingerprint"), chrome.get("akamai_fingerprint")
    if la or ca:
        lines.append(f"akamai_fingerprint: {'equal' if la == ca else 'DIFFER'}")
        if la != ca:
            lines.append(f"  lumen : {la}\n  chrome: {ca}")
    lo, co = lumen.get("header_order"), chrome.get("header_order")
    if lo and co:
        if [h.lower() for h in lo] == [h.lower() for h in co]:
            lines.append("header order: equal")
        else:
            lines.append(f"header order: DIFFER\n  lumen : {lo}\n  chrome: {co}")
    for k in ("ja3_hash", "ja4"):
        lv = (lumen.get("tls") or {}).get(k)
        cv = (chrome.get("tls") or {}).get(k)
        if lv or cv:
            mark = "equal" if lv == cv else "differ"
            lines.append(f"tls.{k}: {mark}  lumen={lv} chrome={cv}")
    return lines or ["no comparable fields extracted"]


def run_arm(kind: str, url: str, exe: Path | None) -> tuple[dict, dict]:
    """Прогон одной руки; возвращает (raw_json, extracted)."""
    session = None
    try:
        if kind == "lumen":
            if exe is None or not exe.is_file():
                sys.exit(f"lumen exe not found: {exe}")
            session = LumenSession(exe, url, {"LUMEN_NO_ADBLOCK": "1"})
        else:
            binary, _ver = find_chrome(None, lumen_chrome_major())
            session = ChromeSession(binary, url)
        body = fetch_body(session, url)
        raw = json.loads(body)
        return raw, (extract_peet(raw) if "peet" in url else extract_bleed(raw))
    finally:
        if session is not None:
            session.close()


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out-dir", type=Path, default=REPO / "docs" / "perf" / "runs")
    ap.add_argument("--date", default=None, help="YYYY-MM-DD; default: today")
    ap.add_argument("--lumen-exe", type=Path, default=REPO / "target" / "dev-release" / "lumen.exe")
    ap.add_argument("--url", default=PEET_URL)
    ap.add_argument("--skip-lumen", action="store_true")
    ap.add_argument("--skip-chrome", action="store_true")
    args = ap.parse_args()

    from datetime import date

    day = args.date or date.today().isoformat()
    args.out_dir.mkdir(parents=True, exist_ok=True)

    url = args.url
    used_fallback = False
    if "peet" in url and not reachable(PEET_URL):
        print(f"peet unreachable — fallback {BLEED_URL}", flush=True)
        url = BLEED_URL
        used_fallback = True

    results = {}
    for kind in ("lumen", "chrome"):
        if (kind == "lumen" and args.skip_lumen) or (kind == "chrome" and args.skip_chrome):
            continue
        raw, extracted = run_arm(kind, url, args.lumen_exe)
        if used_fallback:
            extracted = extract_bleed(raw)
        results[kind] = extracted
        out = args.out_dir / f"{day}-peet-{kind}.json"
        out.write_text(json.dumps(raw, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
        print(f"{kind}: {out}  (source={extracted.get('source')})", flush=True)

    if "lumen" in results and "chrome" in results:
        print("\n── compare ──")
        for line in compare(results["lumen"], results["chrome"]):
            print(" ", line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
