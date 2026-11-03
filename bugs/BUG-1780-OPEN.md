# BUG-1780 — Автологин Сбер ID зацикливается: `/auth/cb/?error=sso_error` перезагружается по кругу

**Статус:** OPEN
**Заведён:** 2026-10-28 (P2, SITES-TRIAGE-2)
**Область:** js / network (`id.sber.ru/CSAFront`, межсайтовый SSO) — не локализовано

## Источник: сайты

afisha-ru (вес 2, `text_len` 11 при 8482 у Chrome), gazeta-ru (вес 2, 1703 при 5154; страница рисуется частично) — **2 сайта, вес кластера 4**. Оба на едином SSO Rambler. Отчёт: [`docs/perf/runs/2026-10-20-compat-triage.md`](../docs/perf/runs/2026-10-20-compat-triage.md) §Разбор blank. Прогон — `compat_crawl.py` живым окном, `LUMEN_NO_ADBLOCK=1`, без прокси, 2026-10-28.

## Симптом

В журнале afisha: `[JS] redirect to sberid for autologin https://id.sber.ru/CSAFront/oidc/authorize.do?…` → `GET …/CSAFront/api/initlow?…` отвечает **400** → переход на `https://www.afisha.ru/auth/cb/?error=sso_error&state=…`, страница (4913 байт) загружается и сразу опять запрашивает тот же адрес — 3+ раза подряд, на экране лого и две точки. Chrome в SSO-цепочку не заходит: главная готова за 0,17 с.

## Что неизвестно

Почему Lumen получает от `initlow` 400 (заголовки, куки `SameSite`, `Origin`, `Referer`/`Sec-Fetch-*` при переходе, сторонний контекст) и почему Chrome не стартует автологин (признак на стороне сайта: cookie/`localStorage` либо `document.referrer`). Первый шаг — снять оба обмена `initlow` и сравнить; затем решить, дефект ли это сети/куки или поведение сайта.

## Как проверить

`python scripts/compat_crawl.py --corpus <файл со строками afisha-ru, gazeta-ru> --env LUMEN_NO_ADBLOCK=1 --no-rerun --chrome-arg=--no-proxy-server`: у обоих `text_len` Lumen ≥ 50 % Chrome, нет повторов `auth/cb/?error=sso_error` в `live.stderr.*.log`.
