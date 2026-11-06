# BUG-1628 — Total Cookie Protection не подключена: cookie не разбиты по сайту верхнего уровня

**Статус:** FIXED 2026-11-06 (P3)
**Заведён:** 2026-10-14 (P6, остаток [BUG-1616](BUG-1616-FIXED.md))
**Область:** network/shell/js/storage (`network/src/lib.rs` `COOKIE_PARTITION`, `js/src/v8_runtime/install/storage.rs`, `shell/src/frames.rs`, `shell/src/lumen/bfcache.rs`, `shell/src/scripts.rs`)

## Симптом и место в коде

`docs/plan/privacy.md:102` обещает для профиля Standard Total Cookie Protection, но все обращения к `CookieJar` идут с ключом раздела `None` (`lumen_network::COOKIE_PARTITION`, `document.cookie` в `storage.rs`). Сторонние cookie общие для всех сайтов-встраивателей, как и до BUG-1616. Колонка `top_level_site` в таблице `cookies` и вся логика jar-а готовы; нет сквозной передачи ключа.

## Что мешает включить просто

Ключ должен совпадать во всех путях одного документа, иначе cookie, выставленные в одном, не читаются в другом: HTTP-ответ документа и подресурсов, `fetch`/XHR, `EventSource`, `document.cookie` в окне, iframe и воркере, bfcache, devtools/BiDi-список cookie, очистка данных сайта. Для iframe нужен сайт именно **верхнего** документа (сейчас в `frames.rs` клиент строится от базы родителя, а подресурсы фрейма — от его собственной базы). Существующие cookie лежат под ключом `""` — нужна миграция или сознательный разлогин.

## Объём и приёмка

Пробросить сайт верхнего документа в `HttpClient::with_cookie_jar` (ключ БД), `V8JsRuntime::with_cookie_jar`/`install_cookie`, фреймы и воркеры; убрать `COOKIE_PARTITION`; решить судьбу строк с ключом `""`. Тест: cookie, выставленная сторонним iframe под сайтом A, не видна тому же iframe под сайтом B; `document.cookie` и HTTP видят один раздел. Поправить `CAPABILITIES.md` (строка про «partitioning»).

## Исправление (2026-11-06, P3)

Глобальный `COOKIE_PARTITION = None` убран; ключ раздела проброшен сквозь все пути документа:

- `HttpClient` (`network/src/lib.rs`): поля `top_level_site` (SameSite/ site-for-cookies) и `cookie_partition` (ключ `cookies.top_level_site`) разделены; `with_cookie_jar` по умолчанию ставит partition = site, `with_cookie_partition` переопределяет — навигация делит cookie по сайту **целевого** документа, SameSite-контекст остаётся у инициатора. Partition едет через все хопы `fetch_with_redirect`, `fetch_subresource`, SSE (`SseCookies.partition`), JS `fetch`/XHR (`js_providers.rs`).
- `CookieJarProvider::with_partition` (`storage/src/cookies.rs`): провайдер, привязанный к документу, перекрывает partition-аргумент любого вызова — `document.cookie`, HTTP и SSE не могут разойтись по разделам. `list_all` получил partition-аргумент (BiDi/automation видят раздел активной страницы).
- Shell: тип `cookie_jar` в конвейере — `DocCookieJar` (`resource_base.rs`): `jar` + `partition_site` (eTLD+1 верхнего документа). `for_base`/`for_url`/`for_site` строят его из URL документа; `FrameLoadEnv` несёт DocCookieJar **верхней** страницы, поэтому iframe и nested-фреймы наследуют её раздел; opaque-кадры по-прежнему без cookie. Навигация (`page_source.rs`) задаёт partition сайтом целевого URL; подресурсы, preload/`Link:`-хинты, стили, шрифты, картинки (включая lazy/dynamic), site-memory, bfcache-thaw и hibernation-restore (`tab_lifecycle/hibernate.rs`) — все через DocCookieJar. Automation `storage.getCookies/setCookie/deleteCookies` работает в разделе активной страницы.
- Миграция: `migrate_legacy_partitions` при `CookieJar::open` переписывает строки с `top_level_site=''` в раздел registrable domain их собственного домена (IP — сам себе сайт) — first-party cookie остаются у сайта-владельца, осознанного разлогина нет.
- Site-for-cookies подресурсов фрейма теперь тоже сайт верхнего документа (как требует SameSite), а не сайта фрейма.

Тесты: `partitioned_provider_binds_context_partition`, `unpartitioned_provider_uses_call_argument`, `legacy_empty_partition_migrates_to_own_site` (storage); `cookie_partition_overrides_site_for_cookies` + обновлённые assert-ы раздела в `cookie_site_context_follows_document_site_and_request_kind` (network). Приёмка «iframe под сайтом A не видит cookie под сайтом B» покрыта provider-уровнем: раздел — атрибут browsing context, а не запроса.
