# BUG-1628 — Total Cookie Protection не подключена: cookie не разбиты по сайту верхнего уровня

**Статус:** OPEN
**Заведён:** 2026-10-14 (P6, остаток [BUG-1616](BUG-1616-FIXED.md))
**Область:** network/shell/js/storage (`network/src/lib.rs` `COOKIE_PARTITION`, `js/src/v8_runtime/install/storage.rs`, `shell/src/frames.rs`, `shell/src/lumen/bfcache.rs`, `shell/src/scripts.rs`)

## Симптом и место в коде

`docs/plan/privacy.md:102` обещает для профиля Standard Total Cookie Protection, но все обращения к `CookieJar` идут с ключом раздела `None` (`lumen_network::COOKIE_PARTITION`, `document.cookie` в `storage.rs`). Сторонние cookie общие для всех сайтов-встраивателей, как и до BUG-1616. Колонка `top_level_site` в таблице `cookies` и вся логика jar-а готовы; нет сквозной передачи ключа.

## Что мешает включить просто

Ключ должен совпадать во всех путях одного документа, иначе cookie, выставленные в одном, не читаются в другом: HTTP-ответ документа и подресурсов, `fetch`/XHR, `EventSource`, `document.cookie` в окне, iframe и воркере, bfcache, devtools/BiDi-список cookie, очистка данных сайта. Для iframe нужен сайт именно **верхнего** документа (сейчас в `frames.rs` клиент строится от базы родителя, а подресурсы фрейма — от его собственной базы). Существующие cookie лежат под ключом `""` — нужна миграция или сознательный разлогин.

## Объём и приёмка

Пробросить сайт верхнего документа в `HttpClient::with_cookie_jar` (ключ БД), `V8JsRuntime::with_cookie_jar`/`install_cookie`, фреймы и воркеры; убрать `COOKIE_PARTITION`; решить судьбу строк с ключом `""`. Тест: cookie, выставленная сторонним iframe под сайтом A, не видна тому же iframe под сайтом B; `document.cookie` и HTTP видят один раздел. Поправить `CAPABILITIES.md` (строка про «partitioning»).
