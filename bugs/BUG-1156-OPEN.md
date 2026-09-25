# BUG-1156 — навигация по ссылке не несёт `Referer`, `document.referrer` новой страницы всегда `''`

**Статус:** OPEN
**Заведён:** 2026-09-25 (P6, при закрытии [BUG-1121](BUG-1121-FIXED.md))
**Область:** network/shell — `crates/network/src/lib.rs:4326` `HttpClient::fetch_page` не читает
`document_context` (его читают только `fetch()`/XHR и подресурсы, GAP-REFERRER);
`crates/shell/src/page_source.rs:240` строит клиент навигации без `with_document_context` и без
URL документа-инициатора; в JS реферер отдаётся из `_lumen_document_referrer`
(`crates/js/src/shim/web_api_shim_mid_b.js`), который сейчас всегда `''`. Документ `<iframe>`
(`crates/js/src/frame_bridge.rs`, фасад `d.referrer`) — тоже `''`.

## Симптом

Страница `/a` со ссылкой `<a href="/b?x=1">`, клик из скрипта; сервер `/b` возвращает
полученный заголовок `Referer` и `document.referrer`. Видимое окно, `--maximized`, без
блокировщика, Chrome 153:

| | `Referer` запроса `/b` | `document.referrer` на `/b` |
|---|---|---|
| Chrome | `http://127.0.0.1:8767/a` | `http://127.0.0.1:8767/a` |
| Lumen | нет | `''` |

GAP-REFERRER закрыл `Referer` для `fetch()`/XHR/`sendBeacon` и всех подресурсов, но не для
навигации верхнего уровня: запрос документа идёт без контекста документа-инициатора. По плану
(`docs/plan/privacy.md:18`) политика по умолчанию — `strict-origin-when-cross-origin`, то есть
при переходе внутри одного источника уходит полный URL, при переходе на другой — только источник.

## Что сделать

1. Протянуть URL и политику реферера документа-инициатора (ссылка, `location.href = …`,
   отправка формы, `window.open`) в `PageSource::Url` и в клиент навигации; `fetch_page` считает
   `Referer` через `referrer_policy::compute_referrer`, как это уже делает `fetch_subresource_inner`.
   Ввод в адресной строке, закладка, перезагрузка — без реферера (HTML LS: `no-referrer` для
   навигаций, запущенных пользователем).
2. Посчитанное значение отдать новому документу: засеять `_lumen_document_referrer` при установке
   DOM (там же, где `_LUMEN_PAGE_URL`) и `referrer` в фасаде документа `<iframe>`.

Критерий: проба выше даёт в Lumen то же, что в Chrome; переход на чужой источник отдаёт только
источник (`http://127.0.0.1:8767/`); `rel=noreferrer` и `referrerpolicy=no-referrer` на `<a>`
дают `''` и запрос без заголовка.
