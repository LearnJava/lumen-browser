#!/usr/bin/env python3
"""Сборка корпуса C (COMPAT-CORPUS-2): 200 RU + 300 global сайтов.

Источник ранга — список Tranco (https://tranco-list.eu, воспроизводим по id списка),
не ручная память. Скрипт ходит в сеть и нужен редко (пересборка корпуса раз в квартал);
ночной прогон читает готовые файлы и сеть для сборки не использует (scripts/corpus_c.py).

Что делает:
  1. Скачивает список Tranco по id (по умолчанию N26LW, 2026-10-13) в .tmp/corpus-c/.
  2. Отбирает кандидатов в порядке ранга Tranco: RU (.ru/.su/.рф + закреплённые бренды) и global
     (всё остальное). Допуск — список CrUX (origin-ы, которые реально открывают пользователи
     Chrome; Tranco полон CDN, трекеров и DNS-доменов, не страниц): global — корзина ≤10k,
     RU — ≤100k; закреплённые бренды допуску не подчиняются. Отбрасывает взрослые/гемблинг по домену и заголовку, дубли владельца
     (один SLD-ярлык = один сайт) и не-HTML (CDN, API-домены).
  3. Для каждого берёт главную и одну внутреннюю страницу (ссылка из главной того же хоста,
     проверена GET 200 text/html). Стена антибота на главной (401/403/429/503 с HTML) —
     сайт остаётся в корпусе: стена и есть предмет наблюдения; внутренняя берётся из
     типовых путей, если не нашлась — пишется пустой.
  4. Пишет docs/perf/corpus-c.tsv (метаданные), corpus-c-ru.txt и corpus-c-global.txt
     (формат `slug url`, читается scripts/perf_audit.py --corpus).

Метаданные не в строках `slug url`: perf_audit.load_corpus отдаёт всё после slug как url.

Запуск: python scripts/corpus_c_build.py [--list-id N26LW] [--workers 48]
Кэш проб лежит в .tmp/corpus-c/probe.json — повторный запуск дозаполняет, а не начинает заново.
Категории не определяются автоматически: берутся из docs/perf/corpus-c-categories.txt.
"""
import argparse
import concurrent.futures as cf
import json
import re
import ssl
import sys
import urllib.error
import urllib.request
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urljoin, urlsplit

sys.stdout.reconfigure(encoding="utf-8")

REPO = Path(__file__).resolve().parent.parent
WORK = REPO / ".tmp" / "corpus-c"
PERF = REPO / "docs" / "perf"
CATEGORIES = PERF / "corpus-c-categories.txt"

N_RU, N_GLOBAL = 200, 300
CRUX_URL = "https://raw.githubusercontent.com/zakird/crux-top-lists/main/data/global/current.csv.gz"
CRUX_MAX_GLOBAL, CRUX_MAX_RU = 10_000, 100_000
UA = ("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) "
      "Chrome/153.0.0.0 Safari/537.36")

# Бренды без RU-доменной зоны и сайты, которые обязаны быть в RU-корпусе независимо от ранга
# (в задаче названы банк, Госуслуги, маркетплейс). Остальные RU-сайты берутся по Tranco.
RU_PINNED = [
    "gosuslugi.ru", "sberbank.ru", "sber.ru", "tbank.ru", "vtb.ru", "alfabank.ru", "ozon.ru",
    "wildberries.ru", "avito.ru", "mos.ru", "nalog.gov.ru", "rzd.ru", "pochta.ru", "hh.ru",
    "kinopoisk.ru", "rutube.ru", "2gis.ru", "megamarket.ru", "dns-shop.ru", "mvideo.ru",
    "cbr.ru", "pfr.gov.ru", "sfr.gov.ru", "lenta.ru", "rbc.ru", "ria.ru", "yandex.ru",
    "vk.com", "ok.ru", "mail.ru", "yandex.com", "kaspersky.ru", "mts.ru", "beeline.ru",
    "megafon.ru", "tinkoff.ru", "raiffeisen.ru", "gazprombank.ru", "sovcombank.ru",
    "detmir.ru", "lamoda.ru", "citilink.ru", "eldorado.ru", "sportmaster.ru", "leroymerlin.ru",
    "vkusvill.ru", "magnit.ru", "5ka.ru", "samokat.ru", "kuper.ru",
]
RU_TLDS = (".ru", ".su", ".xn--p1ai")

# Не берём: взрослые, гемблинг, пиратские каталоги. Домен проверяется подстрокой.
DENY_DOMAIN = re.compile(
    r"porn|xxx|xvideos|xnxx|xhamster|sex|adult|hentai|erotic|nsfw|escort|onlyfans|chaturbate|"
    r"stripchat|livejasmin|casino|1xbet|betting|bet365|melbet|pinup|vulkan|slot|poker|gambl|"
    r"torrent|rutracker|nnmclub|rarbg|thepiratebay"
)
DENY_TITLE = re.compile(
    r"\b18\+|porn|xxx|adult only|только для взрослых|казино|ставки на спорт|букмекер|casino|sportsbook",
    re.IGNORECASE,
)
# Технические домены: не страницы, а инфраструктура (HTML они не отдают, проба отсеет остальное).
DENY_INFRA = re.compile(
    r"(^|\.)(gstatic|googleapis|googleusercontent|googlevideo|ytimg|akamai|akamaiedge|"
    r"cloudfront|azureedge|fastly|cdn|cloudflare-dns|doubleclick|googletagmanager|"
    r"windowsupdate|msedge|msftncsi|apple-dns|icloud-content|fbcdn|twimg|ggpht|yandex(?:net|cloud)|"
    r"yccdn|mycdn|userapi|vkuser|vk-cdn|tiktokcdn|byteoversea|ibytedtos|sentry|appsflyer|"
    r"adjust|amazonaws|awsdns|nflxvideo|nflximg|root-servers|gtld-servers)\b"
)
# Один владелец — один сайт: пары, которые SLD-ярлык не склеивает.
OWNER_ALIAS = {
    "youtu": "youtube", "ya": "yandex", "mycdn": "mail", "googlesyndication": "google",
    "live": "microsoft", "bing": "microsoft", "office": "microsoft", "msn": "microsoft",
    "microsoftonline": "microsoft", "windows": "microsoft", "xbox": "microsoft",
    "tinkoff": "tbank", "fb": "facebook", "instagram": "meta", "whatsapp": "meta", "wa": "meta",
    "amazon": "amazon", "primevideo": "amazon", "twitch": "twitch",
}
# Ручные исключения после просмотра выбора: дубль владельца, регистраторские домены без сайта,
# азартные игры, инфраструктура, которую CrUX считает origin-ом. Причина — после `#`.
EXCLUDE = {
    "cloud.microsoft", "static.microsoft", "office365.com", "azurewebsites.net", "xboxlive.com",
    "safety.google", "android.com", "blogger.com", "wikimedia.org", "shop.app", "it.com",
    "bit.ly",  # дубли владельца (Microsoft, Google, Wikimedia, Shopify), регистратор, сокращатель
    "google.ru", "sber.ru", "sbrf.ru", "com.ru", "net.ru", "org.ru", "pp.ru", "gov.ru", "spb.ru",
    "betcity.ru", "ligastavok.ru", "stoloto.ru",  # азартные игры
    "pixiv.net", "archiveofourown.org",  # пользовательский контент 18+
}
# Ответы, в которых виден отказ антибота или нерешённый редирект на проверку: сайт остаётся в корпусе.
WALL_STATUSES = {301, 302, 303, 307, 308, 401, 403, 418, 429, 439, 498, 503}
INNER_DENY = re.compile(
    r"login|signin|sign-in|signup|sign-up|register|logout|auth|oauth|account|cart|basket|checkout|"
    r"download|subscribe|password|cookie|consent|captcha|\.(?:pdf|zip|exe|dmg|apk|jpg|jpeg|png|gif|"
    r"svg|webp|mp4|mp3|xml|json|rss|txt|css|js)(?:$|\?)",
    re.IGNORECASE,
)
INNER_FALLBACK = ["/about", "/about-us", "/help", "/contact", "/contacts", "/privacy", "/terms",
                  "/o-kompanii", "/about/", "/help/", "/contact/"]


class Links(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.hrefs: list[str] = []
        self.title = ""
        self._in_title = False

    def handle_starttag(self, tag, attrs):
        if tag == "a":
            for k, v in attrs:
                if k == "href" and v:
                    self.hrefs.append(v)
        elif tag == "title":
            self._in_title = True

    def handle_endtag(self, tag):
        if tag == "title":
            self._in_title = False

    def handle_data(self, data):
        if self._in_title and len(self.title) < 200:
            self.title += data


def fetch(url: str, timeout: float = 15.0):
    """GET url -> (status|None, final_url, content_type, body[:400k], error).

    Ошибка проверки сертификата (корни Минцифры/НУЦ, самоподписанная цепочка) не отсекает сайт:
    проба повторяется без проверки, `error` = "tls-unverified". Здесь нужны только статус и HTML,
    доверие к сертификату — предмет браузера, а не сборщика корпуса.
    """
    req = urllib.request.Request(url, headers={
        "User-Agent": UA, "Accept": "text/html,application/xhtml+xml,*/*;q=0.8",
        "Accept-Language": "en-US,en;q=0.9"})
    err = ""
    for ctx in (ssl.create_default_context(), ssl._create_unverified_context()):
        try:
            with urllib.request.urlopen(req, timeout=timeout, context=ctx) as r:
                return r.status, r.geturl(), r.headers.get("Content-Type", ""), r.read(400_000), err
        except urllib.error.HTTPError as e:
            try:
                body = e.read(400_000)
            except Exception:
                body = b""
            ct = e.headers.get("Content-Type", "") if e.headers else ""
            return e.code, e.geturl() or url, ct, body, err
        except urllib.error.URLError as e:
            if isinstance(e.reason, ssl.SSLCertVerificationError) and not err:
                err = "tls-unverified"
                continue
            return None, url, "", b"", err or type(e).__name__
        except Exception as e:  # reset, timeout
            return None, url, "", b"", err or type(e).__name__
    return None, url, "", b"", err


def decode(body: bytes, ctype: str) -> str:
    m = re.search(r"charset=([\w-]+)", ctype, re.IGNORECASE) or re.search(
        rb"<meta[^>]+charset=[\"']?([\w-]+)", body[:4096], re.IGNORECASE)
    enc = "utf-8"
    if m:
        g = m.group(1)
        enc = g.decode("ascii", "ignore") if isinstance(g, bytes) else g
    try:
        return body.decode(enc, "replace")
    except LookupError:
        return body.decode("utf-8", "replace")


def is_html(ctype: str) -> bool:
    return "html" in ctype.lower()


def pick_inner(base_url: str, html: str) -> list[str]:
    """Кандидаты внутренней страницы: ссылки того же хоста, середина списка (не меню входа и не подвал)."""
    p = Links()
    try:
        p.feed(html)
    except Exception:
        pass
    host = urlsplit(base_url).netloc.lower().removeprefix("www.")
    seen, out = set(), []
    for h in p.hrefs:
        if h.startswith(("#", "mailto:", "tel:", "javascript:")):
            continue
        u = urljoin(base_url, h).split("#")[0]
        s = urlsplit(u)
        if s.scheme not in ("http", "https") or s.netloc.lower().removeprefix("www.") != host:
            continue
        if s.path in ("", "/") or len(u) > 140 or INNER_DENY.search(u) or u in seen:
            continue
        seen.add(u)
        out.append(u)
    if not out:
        return []
    mid = len(out) // 3
    return out[mid:] + out[:mid]


def probe(domain: str) -> dict:
    """Главная + внутренняя страница одного домена."""
    res = {"domain": domain, "home": "", "status": None, "error": "", "title": "", "inner": "",
           "inner_status": None}
    html, base = "", ""
    for host in (domain, "www." + domain):
        url = f"https://{host}/"
        st, final, ct, body, err = fetch(url)
        res["status"], res["error"] = st, err
        if st is not None:
            res["home"], res["final"] = url, final
            if is_html(ct) or st in WALL_STATUSES:
                html, base = decode(body, ct), final
                res["ctype_html"] = is_html(ct)
                res["size"] = len(body)
                break
        if err and err != "TimeoutError":
            continue
    if html:
        t = Links()
        try:
            t.feed(html[:200_000])
        except Exception:
            pass
        res["title"] = re.sub(r"\s+", " ", t.title).strip()[:120]
    cands = pick_inner(base, html) if res["status"] == 200 else []
    if not cands and res["home"]:
        root = res["home"].rstrip("/")
        cands = [root + p for p in INNER_FALLBACK]
    for u in cands[:5]:
        st, final, ct, body, err = fetch(u, 12.0)
        if st == 200 and is_html(ct) and len(body) > 800:
            res["inner"], res["inner_status"] = u, 200
            break
    return res


def owner_key(domain: str) -> str:
    parts = domain.split(".")
    sld = parts[-2] if len(parts) >= 2 else parts[0]
    if len(parts) >= 3 and parts[-2] in ("co", "com", "org", "net", "gov", "ac"):
        sld = parts[-3]
    return OWNER_ALIAS.get(sld, sld)


def load_tranco(list_id: str) -> list[str]:
    WORK.mkdir(parents=True, exist_ok=True)
    f = WORK / f"tranco-{list_id}.csv"
    if not f.exists():
        url = f"https://tranco-list.eu/download/{list_id}/1000000"
        print("скачиваю", url)
        with urllib.request.urlopen(url, timeout=180) as r:
            f.write_bytes(r.read())
    rows = []
    for line in f.read_text(encoding="utf-8").splitlines():
        rank, _, dom = line.partition(",")
        if dom:
            rows.append(dom.strip().lower())
    return rows


def load_crux(domains: set[str]) -> dict[str, int]:
    """Корзина CrUX (1000/5000/10000/…) для каждого домена из `domains`: минимум по его origin-ам."""
    import gzip
    WORK.mkdir(parents=True, exist_ok=True)
    f = WORK / "crux-global.csv.gz"
    if not f.exists():
        print("скачиваю", CRUX_URL)
        with urllib.request.urlopen(CRUX_URL, timeout=180) as r:
            f.write_bytes(r.read())
    best: dict[str, int] = {}
    with gzip.open(f, "rt", encoding="utf-8") as fh:
        next(fh)
        for line in fh:
            origin, _, bucket = line.strip().rpartition(",")
            labels = urlsplit(origin).netloc.split(":")[0].lower().split(".")
            for i in range(len(labels) - 1):
                d = ".".join(labels[i:])
                if d in domains:
                    best[d] = min(best.get(d, 10**9), int(bucket))
                    break
    return best


def candidates(rows: list[str], limit_ru: int, limit_global: int):
    """Кандидаты в порядке ранга Tranco с допуском CrUX; запас ×2 — часть отсеется пробой."""
    rank = {d: i + 1 for i, d in enumerate(rows)}
    crux = load_crux(set(rows[:300_000]))
    ru, glob = [], []
    for d in rows:
        if DENY_DOMAIN.search(d) or DENY_INFRA.search(d) or d in EXCLUDE:
            continue
        bucket = crux.get(d, 10**9)
        if d.endswith(RU_TLDS):
            if bucket <= CRUX_MAX_RU and len(ru) < limit_ru:
                ru.append(d)
        elif bucket <= CRUX_MAX_GLOBAL and len(glob) < limit_global:
            glob.append(d)
        if len(ru) >= limit_ru and len(glob) >= limit_global:
            break
    for d in RU_PINNED:
        if d not in ru and d not in EXCLUDE:
            ru.append(d)
        glob = [g for g in glob if g != d]
    return ru, glob, rank


def tier_weight(pos: int) -> int:
    return 3 if pos <= 50 else 2 if pos <= 200 else 1


def select(domains, results, want, rank, pinned=()):
    """Первые want доменов в порядке ранга, прошедшие пробу и не дублирующие владельца."""
    out, owners = [], set()
    ordered = sorted(domains, key=lambda d: (d not in pinned, rank.get(d, 10**9)))
    # закреплённые идут первыми, остальные по рангу; финальный порядок — всё равно по рангу
    for d in ordered:
        r = results.get(d)
        if not r:
            continue
        # Сеть не дошла (DNS/TLS/сброс): с точки сборки сайт может быть закрыт, а с другой
        # открыт — оставляем главную без проверки, это и есть предмет двух точек наблюдения.
        if r["status"] is None:
            r["home"] = r["home"] or f"https://www.{d}/"
        elif r["status"] not in WALL_STATUSES | {200} or not r["home"]:
            continue
        if r["status"] == 200 and not r.get("ctype_html", True):
            continue
        if DENY_TITLE.search(r.get("title", "")):
            continue
        # Редирект на чужой домен считается дублем только у живых страниц: стена антибота
        # часто перекидывает на общий домен проверки (passport.yandex.ru и т.п.).
        fin_owner = owner_key(urlsplit(r.get("final", r["home"])).netloc.removeprefix("www."))
        if r["status"] != 200 or d in pinned:
            fin_owner = owner_key(d)
        key = owner_key(d)
        if key in owners or fin_owner in owners:
            continue
        owners.update({key, fin_owner})
        out.append(d)
        if len(out) >= want:
            break
    out.sort(key=lambda d: rank.get(d, 10**9))
    return out


def load_categories() -> dict[str, str]:
    cats = {}
    if CATEGORIES.exists():
        for line in CATEGORIES.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                d, _, c = line.partition(" ")
                cats[d] = c.strip()
    return cats


def slug_of(domain: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "-", domain.encode("idna").decode("ascii").lower()).strip("-")
    return s


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--list-id", default="N26LW")
    ap.add_argument("--workers", type=int, default=48)
    ap.add_argument("--probe-only", action="store_true", help="только пробы, файлы не писать")
    a = ap.parse_args()

    rows = load_tranco(a.list_id)
    ru, glob, rank = candidates(rows, N_RU * 2, N_GLOBAL * 2)
    cache_f = WORK / "probe.json"
    cache = json.loads(cache_f.read_text(encoding="utf-8")) if cache_f.exists() else {}
    todo = [d for d in ru + glob if d not in cache]
    print(f"кандидатов RU {len(ru)}, global {len(glob)}, в кэше {len(cache)}, проб впереди {len(todo)}")
    with cf.ThreadPoolExecutor(a.workers) as ex:
        for i, r in enumerate(ex.map(probe, todo), 1):
            cache[r["domain"]] = r
            if i % 100 == 0:
                cache_f.write_text(json.dumps(cache, ensure_ascii=False), encoding="utf-8")
                print(f"  {i}/{len(todo)}")
    cache_f.write_text(json.dumps(cache, ensure_ascii=False), encoding="utf-8")
    if a.probe_only:
        return

    sel_ru = select(ru, cache, N_RU, rank, set(RU_PINNED))
    sel_gl = select(glob, cache, N_GLOBAL, rank)
    print(f"отобрано RU {len(sel_ru)}/{N_RU}, global {len(sel_gl)}/{N_GLOBAL}")
    cats = load_categories()
    tsv = ["# slug\tсегмент\tпозиция\tвес\tранг-tranco\tкатегория\tисточник\tстатус-главной\tглавная\tвнутренняя"]
    files = {}
    for seg, sel in (("ru", sel_ru), ("global", sel_gl)):
        lines = [
            f"# Корпус C, сегмент {seg} ({len(sel)} сайтов): главная + одна внутренняя страница.",
            f"# Ранг: Tranco {a.list_id}; вес 3/2/1 по позиции в сегменте (1-50 / 51-200 / остальные).",
            "# Метаданные (категория, ранг, вес) — в docs/perf/corpus-c.tsv. Формат строк: <slug> <url>.",
            "# Сгенерировано scripts/corpus_c_build.py — руками не править, пересобирать.",
            "",
        ]
        for pos, d in enumerate(sel, 1):
            r = cache[d]
            slug = slug_of(d)
            src = f"tranco:{a.list_id}" + ("+pinned" if seg == "ru" and d in RU_PINNED else "")
            tsv.append("\t".join([
                slug, seg, str(pos), str(tier_weight(pos)), str(rank.get(d, 0)),
                cats.get(d, "?"), src, (f'{r["status"]}/{r["error"]}' if r["status"] and r["error"] else str(r["status"] or "unreachable")),
                r["home"], r["inner"]]))
            lines.append(f"{slug:<28}{r['home']}")
            if r["inner"]:
                lines.append(f"{slug + '--inner':<28}{r['inner']}")
        files[PERF / f"corpus-c-{seg}.txt"] = "\n".join(lines) + "\n"
    files[PERF / "corpus-c.tsv"] = "\n".join(tsv) + "\n"
    for f, text in files.items():
        f.write_text(text, encoding="utf-8", newline="\n")
        print("записан", f.relative_to(REPO))
    miss = [d for d in sel_ru + sel_gl if d not in cats]
    print(f"без категории: {len(miss)} (заполни docs/perf/corpus-c-categories.txt)")


if __name__ == "__main__":
    main()
