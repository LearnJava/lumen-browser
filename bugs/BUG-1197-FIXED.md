# BUG-1197 — blob-URL без происхождения: `blob:lumen/N` вместо `blob:<origin>/<uuid>`

**Статус:** FIXED 2026-09-28 (P6)
**Заведён:** 2026-09-27 (P1, при закрытии [BUG-585](BUG-585-FIXED.md) / GAP-ORIGIN)
**Область:** js (`crates/js/src/shim/web_api_shim_mid_c2.js:4` — `URL.createObjectURL`,
`crates/js/src/worker.rs` — разбор `blob:lumen/` при старте воркера и в `importScripts`)

## Симптом

`URL.createObjectURL(blob)` отдаёт `blob:lumen/1`, `blob:lumen/2`, … По File API §8.3
(«generate a new blob URL») это `blob:` + сериализованное происхождение создателя + `/` + UUID,
например `blob:http://localhost:18300/4b2c…`. У `blob:lumen/1` внутреннего URL с
http(s)-схемой нет, и по URL Standard §6.1 его происхождение непрозрачное.

Видно через GAP-ORIGIN: WPT `html/browsers/origin/api/origin-from-worker.window.html`,
сабтест «Comparison of `Origin.from(Worker)` tuple origins» — воркер из
`URL.createObjectURL(new Blob([src]))` получает `location.href === 'blob:lumen/N'`, и
`Origin.from(globalThis)` в нём непрозрачный (`isOpaque: true`), а должен быть tuple-origin
создавшей страницы. Тот же корень у `new URL(URL.createObjectURL(b)).origin` (сейчас `''`,
должно быть происхождение страницы).

## Что сделать

Генерировать `blob:<origin документа>/<uuid>`; ключ хранилища (`_object_url_store`,
`_lumen_blob_url_entry`) и все места, распознающие префикс `blob:lumen/` (воркер,
`importScripts`, fetch/XHR/`<script src>` из BUG-1126), перевести на новый формат.

## Исправление (2026-09-28, P6)

- `URL.createObjectURL` (`crates/js/src/shim/web_api_shim_mid_c2.js`) генерирует
  `blob:` + `location.origin` (для непрозрачного — `null`) + `/` + UUID v4
  (`crypto.randomUUID`, запасной путь — `Math.random`). Счётчик `_object_url_seq` удалён.
- `URL.prototype.origin` (`crates/js/src/js_url.rs`) для `blob:` берёт сериализацию
  происхождения по URL Standard (`Url::origin_serialization`, новый метод в `lumen-core`):
  происхождение вложенного URL, `null` для непрозрачного. Раньше `Url::origin` смотрел
  на authority, которой у blob-URL нет, и отдавал `''`. Отсюда же `location.origin`
  blob-воркера (`_lumen_make_worker_location`).
- Все места, узнававшие префикс `blob:lumen/` (конструкторы `Worker`/`SharedWorker`,
  `importScripts` через `WorkerBlobStore`, CSP-пропуск `import_scripts_csp_blocked`,
  `<track src>`), теперь узнают любой `blob:` и ищут запись через `_lumen_blob_url_entry`
  (без фрагмента, File API §8.3); Rust-сторона `resolve_import_url` тоже отрезает фрагмент.
- Тесты: `js_url::tests::blob_url_origin_is_its_inner_urls_origin`,
  `v8_page_visibility_beacon::url_create_object_url_carries_document_origin`,
  `v8_webworker::worker_from_blob_url_has_the_creating_page_origin` (сценарий WPT
  `origin-from-worker`: в blob-воркере `location.origin` — происхождение страницы,
  `Origin.from(globalThis)` — tuple, same-origin со страницей). WPT живьём не перемерялся.
