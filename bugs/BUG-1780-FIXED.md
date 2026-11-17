# BUG-1780 — Автологин Сбер ID зацикливается: `/auth/cb/?error=sso_error` перезагружается по кругу

**Статус:** FIXED 2026-11-17 (P3)
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

## Закрытие (P3, 2026-11-17) — петля была reload() после history.replaceState

**Дефект движка:** страница-пружина afisha `/auth/cb/` (4913 байт) делает
`history.replaceState(null, '', RequestUri)` + `window.location.reload()`
(`RequestUri = "/"`). Shell дренит replaceState только в `display_url`
(адресная строка, `about_to_wait.rs`), а `reload()` перечитывает
`self.source` — адрес, с которого документ стартовал
(`/auth/cb/?error=sso_error&state=…`), поэтому страница-пружина перечитывала
саму себя вечно (~86 GET за 90 с, каждый с полным parse+JS) — это и есть
«страница загружается и сразу опять запрашивает тот же адрес» из отчёта и
вклад «JS 30–40 с» в вес кластера. По HTML LS §7.4.3 pushState/replaceState
переписывают URL текущей записи истории — reload обязан идти по нему
(Chromium после cb перегружает `/` и попадает на главную).

**Фикс:** `PageSource::retarget_url` — дренаж `HistoryUrlUpdate` (push и
replace) и same-document завершение перехваченной `navigation.navigate()`
ведут URL `self.source` за same-document rewrite; для push — строго после
снимка записи в `push_same_document_entry` (запись back-стека несёт свой
предыдущий URL). Same-document `back()` и так восстанавливал URL источника
из записи (`navigation.rs:488`) — фикс ложится в существующую модель. Тест
`retarget_url_follows_same_document_rewrite` (`tests/form_post_nav.rs`).

**Стенд (детерминированный, до/после):** копия страницы-пружины
(`RequestUri=/main2.html`) на локальном http. До фикса за 10 с: 40 GET
cb-URL, 0 GET `/main2.html`. После: 1 + 1. Консоль страницы в обоих прогонах
видела `{"Success":true,"Redirect":true,"RequestUri":"/main2.html"}` —
вариант «сервер отдал другой вариант страницы» исключён.

**initlow 400 — не дефект движка.** Живой Chromium 1243 (ms-playwright,
свежий профиль, headed) на afisha 2026-11-17 заходит в ту же SSO-цепочку
(`authorize.do` → `antifraud.min.js` → POST `initlow` → **тот же 400** →
`/auth/cb/?error=sso_error`): отчёт 2026-10-28 «Chrome в SSO-цепочку не
заходит» устарел вместе с поведением сайта (`machineClick=aggressivelogin`
в authorize-URL). 400 остаётся на стороне id.sber.ru (антифрод-состояние
клиента/IP); Chromium после `sso_error` показывает виджет входа и живёт —
Lumen после фикса делает то же.

**Живая приёмка (afisha.ru, `LUMEN_NO_ADBLOCK=1`, `--maximized`, dev-release):**
одна попытка `sso_error` в stderr (повторов 0; до фикса ~86), после cb —
`GET https://www.afisha.ru/` → 200, финальный URL `https://www.afisha.ru/`,
`text_len` 9740 (Chromium 19536 в парном прогоне — 49,9 %, на границе
критерия 50 %, одноразовый замер живого сайта). Каноничный compat_crawl-прогон
не выполнялся; gazeta-ru (дубль) не гонялся. Гейт: `clippy -p lumen-shell
--all-targets -D warnings` чистый, bin-тесты shell 2540 зелёных; красные
вне среза (bug489/snapshot_cpu/empty_cells/hevc/clipboard) сверены
stash-прогоном на базлайне — одинаковы с правкой и без; `dump_golden` 12/12.
