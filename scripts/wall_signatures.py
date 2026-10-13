#!/usr/bin/env python3
"""Сигнатуры «стен» сайта: ответ, который не пускает клиента, но не является
ошибкой движка (COMPAT-AB-IDENTITY-HARNESS, report.md §7.6 п.9).

Стена бывает двух видов:
  * `hard` — главный документ вернул отказ (401/403/429/503/RST) и в ответе
    видна подпись вендора антибота (Cloudflare, DataDome, Akamai, PerimeterX,
    Imperva);
  * `soft` — статус 200, но вместо страницы заглушка («Unsupported browser»,
    «Upgrade your browser», флаг `KA-is-unsupported-browser: true`). Бинарная
    проверка по статусу такую стену не видит (BUG-1113, khanacademy).

Модуль общий для `compat_ab_chrome.py`, `header_matrix.py` и (позже) краулера
COMPAT-CORPUS-1; сам ничего не скачивает.

    python scripts/wall_signatures.py --status 403 --file body.html [--header 'Server: cloudflare']
    python scripts/wall_signatures.py --selftest
"""

from __future__ import annotations

import argparse
import re
import sys

for _stream in (sys.stdout, sys.stderr):
    if hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")

# Статусы, на которых вендорская подпись означает жёсткую стену.
REFUSAL_STATUSES = (401, 403, 429, 503)
# Общие фразы («access denied») считаем заглушкой только в коротком теле.
GENERIC_BLOCK_MAX_BODY = 20_000
# Без отрисованного текста фразы ищем только в коротком сыром теле.
PHRASE_MAX_BODY = 60_000

# (id, вендор, где ищем, регулярка). `where`: "body" | "header" (строка
# `имя: значение`, имя в нижнем регистре) | "any" (тело или заголовки).
# Подписи вендоров — жёсткая стена только вместе с отказным статусом
# (и `cf-mitigated`, который сам по себе означает challenge).
VENDOR_SIGNATURES: tuple[tuple[str, str, str, str], ...] = (
    ("cloudflare-mitigated", "cloudflare", "header", r"^cf-mitigated:\s*challenge"),
    ("cloudflare-just-a-moment", "cloudflare", "body", r"<title>\s*Just a moment\.\.\.|Checking if the site connection is secure"),
    ("cloudflare-attention", "cloudflare", "body", r"Attention Required! \| Cloudflare|cf-error-details"),
    ("cloudflare-chl", "cloudflare", "body", r"/cdn-cgi/challenge-platform/|cf-chl-(?:bypass|opt|widget)|challenges\.cloudflare\.com/turnstile"),
    ("datadome-host", "datadome", "body", r"captcha-delivery\.com|geo\.captcha-delivery"),
    ("datadome-header", "datadome", "header", r"^x-datadome|^set-cookie:.*\bdatadome="),
    ("akamai-denied", "akamai", "body", r"<H1>Access Denied</H1>|You don't have permission to access .{0,200}on this server\.\s*<P>\s*Reference&#32;#"),
    ("akamai-reference", "akamai", "body", r"Reference\s*(?:&#32;|#)\s*#?\s*\d+\.[0-9a-f]+\.\d+\.[0-9a-f]+"),
    ("akamai-server", "akamai", "header", r"^server:\s*AkamaiGHost"),
    ("perimeterx", "perimeterx", "body", r"px-captcha|_pxCaptcha|captcha\.px-cdn\.net|Press\s*&amp;\s*Hold|Press & Hold"),
    ("imperva", "imperva", "any", r"Incapsula incident ID|_Incapsula_Resource|^x-iinfo:"),
    ("sucuri", "sucuri", "body", r"Sucuri WebSite Firewall|Access Denied - Sucuri"),
)

# Мягкие стены: страница отдана 200-ом, но это заглушка. Ищем в теле (и в
# отрисованном тексте DOM, если вызывающий его передал).
SOFT_SIGNATURES: tuple[tuple[str, str, str], ...] = (
    ("ka-unsupported-flag", "unsupported-browser", r"\"?KA-is-unsupported-browser\"?\s*[:=]\s*true"),
    ("client-challenge", "js-challenge", r"<title>\s*Client Challenge\s*</title>|/_fs-ch-[A-Za-z0-9]+/"),
    ("unsupported-browser-en", "unsupported-browser", r"\bunsupported browser\b|\bbrowser (?:is )?not supported\b|\bupgrade your browser\b|\bupdate your browser\b|\byour browser is (?:out of date|outdated|no longer supported)\b|\bswitch to a supported browser\b|\bplease use a (?:modern|supported) browser\b"),
    ("unsupported-browser-ru", "unsupported-browser", r"браузер (?:устарел|не поддерживается)|ваш браузер (?:устарел|не поддерживается)|обновите (?:ваш )?браузер|неподдерживаемый браузер|используйте (?:современный|другой) браузер"),
    ("blocked-generic-en", "blocked", r"\b(?:access|request) (?:denied|blocked)\b|\bunusual traffic\b|\bverify (?:that )?you are (?:a )?human\b|\bare you a robot\b"),
    ("blocked-generic-ru", "blocked", r"доступ (?:запрещ[её]н|ограничен)|подозрительн\w+ (?:трафик|активность)|подтвердите,? что вы не робот"),
)

_VENDOR_RE = tuple((i, v, w, re.compile(p, re.I | re.M)) for i, v, w, p in VENDOR_SIGNATURES)
_SOFT_RE = tuple((i, k, re.compile(p, re.I)) for i, k, p in SOFT_SIGNATURES)


def _header_block(headers: dict[str, str] | list | None) -> str:
    """Заголовки → текст `имя: значение`, имя в нижнем регистре."""
    if not headers:
        return ""
    items = headers.items() if isinstance(headers, dict) else headers
    return "\n".join(f"{str(k).lower()}: {v}" for k, v in items)


def classify_wall(
    status: int | None,
    headers: dict[str, str] | list | None = None,
    body: str = "",
    net_error: str | None = None,
    visible_text: str | None = None,
) -> dict:
    """Определить стену.

    Возвращает `{"kind": none|hard|soft|refused, "vendor": str|None,
    "signals": [id, ...]}`:
      * `hard` — отказный статус (или сетевой сброс) + подпись вендора;
      * `refused` — отказный статус без подписи (антибот, но неопознанный);
      * `soft` — статус 2xx и сигнатура заглушки в теле;
      * `none` — ничего не найдено.
    Тело длиннее 400 КБ обрезается: подписи лежат в начале страницы.
    `visible_text` — отрисованный текст страницы (`innerText`): фразы вроде
    «Upgrade your browser» ищем в нём, потому что в сыром HTML большого сайта
    они встречаются в JS-бандлах. Без него фразы ищутся в теле до
    `PHRASE_MAX_BODY` байт; флаг `KA-is-unsupported-browser` — в любом теле.
    """
    hdr = _header_block(headers)
    text = body[:400_000] if body else ""
    signals: list[str] = []
    vendors: list[str] = []
    for sid, vendor, where, rx in _VENDOR_RE:
        hay = hdr if where == "header" else text if where == "body" else hdr + "\n" + text
        if rx.search(hay):
            signals.append(sid)
            if vendor not in vendors:
                vendors.append(vendor)

    refusal = (status in REFUSAL_STATUSES) or (status is None and bool(net_error))
    if refusal and vendors:
        return {"kind": "hard", "vendor": vendors[0], "signals": signals}
    if "cloudflare-mitigated" in signals:
        return {"kind": "hard", "vendor": "cloudflare", "signals": signals}
    soft_kind = None
    phrase_hay = visible_text if visible_text is not None else (text if len(text) <= PHRASE_MAX_BODY else "")
    for sid, kind, rx in _SOFT_RE:
        hay = text + " " + (visible_text or "") if sid.endswith("-flag") else phrase_hay
        # «Access denied» в тексте большой страницы — статья, а не заглушка.
        if kind == "blocked" and len(hay) > GENERIC_BLOCK_MAX_BODY:
            continue
        if rx.search(hay):
            signals.append(sid)
            soft_kind = soft_kind or kind
    if refusal:
        return {"kind": "refused", "vendor": None, "signals": signals}
    if soft_kind and (status is None or 200 <= status < 400):
        return {"kind": "soft", "vendor": soft_kind, "signals": signals}
    return {"kind": "none", "vendor": None, "signals": signals}


def _selftest() -> int:
    cases = [
        (403, {"Server": "AkamaiGHost"}, "<H1>Access Denied</H1> Reference&#32;#18.abcd.123.456", "hard", "akamai"),
        (403, {}, "<title>Just a moment...</title>", "hard", "cloudflare"),
        (401, {}, '<iframe src="https://geo.captcha-delivery.com/captcha/">', "hard", "datadome"),
        (200, {}, 'window.__KA_DATA__={"KA-is-unsupported-browser": true}', "soft", "unsupported-browser"),
        (200, {}, "<h1>Upgrade your browser</h1>", "soft", "unsupported-browser"),
        (200, {}, "<title>Client Challenge</title><link href='/_fs-ch-1T1wmsGaOgGaSxcX/assets/styles.css'>", "soft", "js-challenge"),
        (200, {}, "<h1>Ваш браузер устарел</h1>", "soft", "unsupported-browser"),
        (403, {}, "Forbidden", "refused", None),
        (200, {}, "<html><body>обычная страница, KA-is-unsupported-browser: false</body></html>", "none", None),
        (None, {}, "", "hard", None),
    ]
    bad = 0
    for status, h, body, kind, vendor in cases:
        res = classify_wall(status, h, body, net_error="RST_STREAM" if status is None else None)
        # последний случай — сброс без подписи: refused, а не hard
        want_kind = "refused" if (status is None and kind == "hard") else kind
        if res["kind"] != want_kind or (vendor and res["vendor"] != vendor):
            bad += 1
            print(f"FAIL {status} {body[:40]!r}: ждали {want_kind}/{vendor}, получили {res}")
    print("selftest:", "FAIL" if bad else f"OK ({len(cases)} случаев)")
    return 1 if bad else 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--status", type=int)
    ap.add_argument("--file", help="файл с телом ответа")
    ap.add_argument("--header", action="append", default=[], help="'Имя: значение', можно несколько раз")
    ap.add_argument("--selftest", action="store_true")
    args = ap.parse_args()
    if args.selftest:
        return _selftest()
    body = open(args.file, encoding="utf-8", errors="replace").read() if args.file else ""
    hdrs = dict(h.split(":", 1) for h in args.header)
    print(classify_wall(args.status, {k.strip(): v.strip() for k, v in hdrs.items()}, body))
    return 0


if __name__ == "__main__":
    sys.exit(main())
