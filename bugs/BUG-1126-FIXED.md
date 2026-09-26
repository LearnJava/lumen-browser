# BUG-1126 — `blob:` URL не загружается: `fetch()`, XHR и `<script src=blob:>` дают network error

**Статус:** FIXED 2026-09-26 (P6)
**Заведён:** 2026-09-24 (P2, разбор совместимости после прогона top100-foreign: 48 сайтов с поломкой отрисовки, видимое окно `--maximized` против Chrome 153, **без блокировщика** (`LUMEN_NO_ADBLOCK=1`); [журнал](../docs/perf/journal.md) §2026-09-24 compat). Передан P6 по решению пользователя.
**Область:** network (`crates/network/src/lib.rs:479-483` `require_http_scheme` → `unsupported scheme: blob`) + js (`crates/js/src/shim/web_api_shim_mid_c.js:187` `_object_url_store` — в `fetch` не используется)

## Симптом

- zoom: `fetch error: network error: unsupported scheme: blob` →
  `[ERROR__Campaign/DataFetchingPlugin] Failed to load CDN campaigns TypeError: fetch: network error
  for blob:lumen/1` — нет `#reviews-card`, `#zoom-workplace`, `#learn-more-about-zoom-team-chat`.
- bing: `script load failed: blob:lumen/2: TypeError: fetch: network error for blob:lumen/2`.

`blob:` сейчас разрешается только в воркерах (`worker.rs`, `shared_worker.rs`, `sw_worker.rs`) и в
`<track>` (`video_element.js:478-488`); `fetch` страницы идёт в сеть и получает «unsupported scheme».

## Репро

Файлы — `.tmp/compat/` в worktree аудита (`.claude/worktrees/perf-base`), отдаются `python -m http.server` с `127.0.0.1`; сравнение — `.tmp/compat/probe.py both <url>` (видимое окно, без блокировщика).

`g2/blobscript.html`:

```html
<!doctype html><html><head><title>blob script</title></head><body>
<pre id="out"></pre>
<script>
var r = {};
window.__r = r;
var u = URL.createObjectURL(new Blob(['window.__blobRan = 42;'], {type: 'text/javascript'}));
r.url = u;
var s = document.createElement('script');
s.src = u;
s.onload = function(){ r.onload = true; r.ran = window.__blobRan; };
s.onerror = function(){ r.onerror = true; };
document.head.appendChild(s);
fetch(u).then(function(x){ return x.text(); }).then(function(t){ r.fetchText = t; }, function(e){ r.fetchErr = String(e); });
setTimeout(function(){ document.getElementById('out').textContent = JSON.stringify(r); }, 1500);
</script></body></html>
```

**Результат:** Lumen: URL `blob:lumen/1`, `script.onerror`, `TypeError: fetch: network error for blob:lumen/1` и для `fetch()`, и для `<script src>`; XHR из `.tmp/compat/g4/repro-fetch.html` — `error`. Chrome: `onload`, `__blobRan=42`, `fetch` возвращает тот же текст, XHR ok.

## Что сделать

Fetch §4.2 «scheme fetch», ветка `blob`: взять запись из blob URL store (File API §8.3),
ответ 200 с `Content-Type` = тип Blob и телом из байт; отозванный URL → network error. Разрешать
`blob:` в шиме до ухода в сеть — для `fetch`, XHR и загрузки `<script src>`. Критерий: репро даёт
`__blobRan=42` и текст через `fetch`; zoom перемерить.

## Исправление (2026-09-26, P6)

Корень подтверждён: хранилище blob URL живёт только в JS (`_object_url_store`,
`web_api_shim_mid_c2.js`), а `fetch()`/XHR отдавали URL сетевому слою как есть.

- `_lumen_blob_url_entry(url)` (`web_api_shim_mid_c2.js`) — File API §8.3 «resolve a blob URL»:
  ключ без фрагмента, отозванный/чужой URL → `null`.
- `fetch()` (`web_api_shim_mid_b3.js`): ветка `blob` Fetch §4.2 до ухода в сеть — только `GET`,
  иначе и для отозванного URL `TypeError`; ответ — `_lumen_response_from_blob`
  (`fetch_body_shim.js`): 200 `OK`, `Content-Type` = тип Blob, `Content-Length` = размер, тело — копия байт.
- XHR (`xhr.rs`): та же ветка для синхронного и асинхронного `send()`; сборка ответа вынесена в
  общий `deliver()`, сетевой путь идёт через него же.
- Вставленный `<script src=blob:>` грузится через `fetch()` и чинится им; записанный
  `document.write` (`_lumen_dw_start_fetch`, `web_api_shim_mid.js`) шёл мимо `fetch` прямо в
  сетевой мост — ему добавлена та же ветка.

Проверка: юнит-тесты `fetch_blob_url_*`, `xhr_blob_url_loads_in_sync_and_async_mode`,
`inserted_script_with_blob_src_runs_and_fires_load` (`crates/js/src/dom/tests/v8_whatwg_streams.rs`).
Бинарь dev-release, `--dump-layout` по репро из заявки: `fetch` отдаёт `window.__blobRan = 42;`,
`document.write('<script src=blob:…>')` исполняется. Вставленный скрипт в `--dump-layout` не
проверить: headless не крутит `setTimeout`, а загрузка вставленного скрипта стартует из него —
тот же `pending` у обычного http-скрипта; покрыт юнит-тестом с `_lumen_tick_timers`.
zoom живьём не перемерен.
