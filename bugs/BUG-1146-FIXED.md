# BUG-1146 — Блокировщик: опция `$domain=` у правил EasyList игнорируется — первосторонние скрипты блокируются правилами для чужих сайтов

**Статус:** FIXED 2026-09-28 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** network (`crates/network/src/filter/easylist.rs:233-235` `parse_options`: `key == "domain"` → только `narrows_beyond_domain`, домен страницы не сверяется; `crates/core/src/ext.rs:177` — в `RequestContext` нет хоста документа)

## Симптом

Только при включённом блокировщике (с 2026-09-23 он по умолчанию выключен, и все замеры идут без
него). В исходном прогоне top100 он ломал сайты правилами, которые должны действовать только на
перечисленных в `domain=` страницах:

| Сайт | Заблокировано | Правило |
|---|---|---|
| whatsapp | `https://static.whatsapp.net/rsrc.php/v4/yw/r/wK5kjw4kgtM.js` (определяет `require`/`requireLazy`) | `/^https?:\/\/.*\/[a-z0-9A-Z_]{2,15}\.(php\|jx\|jsx\|1ph\|jsf\|jz\|jsm\|j$)/$script,subdocument,domain=3movs.com\|4kporn.xxx\|…` |
| microsoft | `c.s-microsoft.com/…/script.jsx` | то же |
| duolingo | все бандлы `d35aaqx5ub95lt.cloudfront.net` (app, manifest, polyfills, css) | `\|\|cloudfront.net^$domain=buffsports.io\|chessgames.com\|…`, `\|\|cloudfront.net^$script,domain=gentside.com` (EasyPrivacy) |

Без блокировщика whatsapp даёт 1314 узлов против 1310 в Chrome, с ним — 1004. Правило с
`domain=naver.com` на naver применилось корректно (совпадение случайное). BUG-989 снял блокировку
только для навигации верхнего уровня; тест `domain_option_ignored_not_narrowing` закрепляет
текущее поведение.

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

HTML-репро нет. Доказательство — сверка правил скриптом с тем же алгоритмом матчинга по `target/dev-release/data/adblock/lists/easylist.txt` (`.tmp/compat/g1/emul.py` в worktree аудита) плюс A/B с блокировщиком и без.

**Результат:** whatsapp: блокировщик вкл. — 1004 узла, `requireLazy is not defined`; выкл. — 1314 узлов (Chrome 1310).

## Что сделать

Adblock Plus filter syntax: `domain=a.com|~b.com` ограничивает правило страницами (документом,
инициировавшим запрос) из списка, `~` — исключения. Передать хост документа в `RequestContext` и
сверять его с `domain=`; правило без совпадения не применяется. Тест
`domain_option_ignored_not_narrowing` заменить. Критерий: с включённым блокировщиком whatsapp,
duolingo, microsoft грузят свои бандлы, правило naver по-прежнему срабатывает на naver.

## Исправление (2026-09-28, P6)

- `RequestContext` (`crates/core/src/ext.rs`) получил `document_host: Option<&str>` — ASCII-хост
  документа, от имени которого идёт запрос. `HttpClient` берёт его из `with_document_context`
  (его уже ставят все субресурсные клиенты страницы и JS-`fetch`/XHR) и передаёт в
  `fetch_with_redirect` рядом с `top_level_site`; по редиректам он переходит без изменений.
  `top_level_site` для этого не годился: это registrable-домен, и в проде он везде `None`.
- `easylist.rs`: `domain=a.com|~sub.a.com` разбирается в `DomainCondition` по семантике ABP:
  хост страницы или его родитель совпадает с записью, решает самая специфичная запись, `~`
  исключает; без положительных записей правило действует везде, кроме исключённых. Запись-сущность
  uBlock `name.*` (в списках одна, `wayfair.*`) тоже поддержана. Условие действует и на
  `@@`-исключения. Если хост неизвестен, правило срабатывает консервативно, как type/party.
  Исключение навигации верхнего уровня для правил с `domain=` (BUG-989) сохранено.
- Тест `domain_option_ignored_not_narrowing` заменён шестью тестами `domain_option_*`
  (репро-правило whatsapp, `||cloudfront.net^$domain=…`, `~`-исключения и самая специфичная
  запись, сущность, неизвестная страница, `@@…$domain=`).

**Проверка вживую** (`--maximized --mcp-live-port`, блокировщик **включён**, списки
EasyList + EasyPrivacy, 107 205 правил):

| Сайт | Результат |
|---|---|
| www.whatsapp.com | 1315 узлов (Chrome 1310, до правки с блокировщиком 1004); ни одного `blocked: easylist` |
| www.duolingo.com | 4267 узлов, бандлы `d35aaqx5ub95lt.cloudfront.net` грузятся; блокируются только трекеры (`excess.duolingo.com/batch`, GTM, OneTrust geo) |
| www.naver.com | `||ntm.pstatic.net/scripts/ntm_$domain=naver.com` по-прежнему срабатывает (`ntm_27291e35193e.js` заблокирован) |
| www.microsoft.com | `script.jsx` страница больше не запрашивает; блокируются только аналитика (`exp-analytics…min.js`) и `rum.hlx.page` |

microsoft не доходит до `document_ready` за 45 с и **без** блокировщика: модульный водопад
`reimagine-resources/*.js` ещё идёт. Это не связано с фильтром.
