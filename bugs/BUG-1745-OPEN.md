# BUG-1745 — Cache API в окне: нет класса `Cache`, тело хранится строкой «78,101,…», `statusText` «OK», не работают опции match

**Статус:** OPEN
**Заведён:** 2026-10-27 (P2, WPT-SW-SCOUT)
**Область:** js (`crates/js/src/shim/web_api_shim_mid_b.js` — `_lumen_build_response`, `_lumen_build_cache_object`, `var caches`; натив `_lumen_cache_*`; `crates/storage` — `cache_store`)

## Симптом

```js
const c = await caches.open("x");
c instanceof Cache;                       // ReferenceError: Cache is not defined
await c.put("/a", new Response("bread x 2"));
const r = await c.match("/a");
r.statusText;                             // "OK" — должно быть ""
await r.text();                           // "98,114,101,97,100,…" — коды байтов через запятую
r.url;                                    // "" — должно быть абсолютный URL запроса
await c.add();                            // resolve — должно быть TypeError
```

- Кэш — простой объект из `_lumen_build_cache_object` (`web_api_shim_mid_b.js:347`), глобала `Cache` и прототипа нет (`cache-storage`: «Cache is not defined»).
- `put` передаёт в натив `Uint8Array` (`_lumen_cache_put`, `v8_runtime/install/net.rs:167`, `body: Vec<u8>`), но на обратном пути тело, судя по выводу, приходит обычным JS-массивом, и `_lumen_build_response` (`:334`) зовёт `new Response(array)` — массив сериализуется в строку (`cache-storage-buckets`: «expected "bread x 2" but got "101,103,103,…"»). Точное место конверсии не локализовано — проверить нативы `_lumen_cache_match*`.
- `_lumen_build_response` подставляет `statusText: 'OK'` по умолчанию и при пустой строке (`m.statusText || 'OK'`, то же в `put`); `url`, `type`, `redirected` не восстанавливаются (`cache-match`, `cache-matchAll`, `cache-storage-match`, `cache-put`: «Attributes differ: statusText / url»).
- Опции `ignoreSearch`, `ignoreMethod`, `ignoreVary` игнорируются; HEAD-запрос, фрагмент URL и заголовок `Vary` не учитываются (`cache-keys`: «with ignoreSearch should ignore the search parameters … expected 2 but got 1»).
- `keys()` не в порядке вставки; `CacheStorage.keys()` после `delete` возвращает удалённые имена, `has()`/`delete()` сразу после `open()` дают `false` (`cache-storage-keys`).
- Нет отказов спецификации: `put` с ответом 206, `addAll` с !ok-ответом, не-GET запрос, вызов без аргументов, `add` после `AbortSignal.abort()` (`cache-add`, `cache-delete`, `cache-abort` — TIMEOUT). `bodyUsed` у записанного ответа не выставляется.
- Iframe с `sandbox` без `allow-same-origin` получает доступ к `caches` (`sandboxed-iframes`: «expected "denied" but got "allowed"»).

## Как найдено

WPT-SW-SCOUT, кластер E (`docs/wpt-vendor-notes/service-workers.md`): 17 id в `service-workers/cache-storage/` (OK 12, TIMEOUT 5), потеряно 14.3 балла (4.3 п.п. категории). Представители: `cache-storage/cache-match.https.any.html`, `cache-storage/cache-storage-buckets.https.any.html`.

## Что делать

1. Классы `Cache` и `CacheStorage` с прототипами (WebIDL-форма), `caches instanceof CacheStorage`.
2. Нативы `_lumen_cache_match*` возвращают тело как `Uint8Array`/`ArrayBuffer`, не как массив чисел.
3. Хранить и восстанавливать `status`, `statusText` (как есть, включая ""), заголовки, `url`, `type`; ключ — `Request` с методом и заголовками для `Vary`.
4. Алгоритм «Query Cache» (Service Workers §Cache): ignoreSearch/ignoreMethod/ignoreVary, снятие фрагмента, порядок вставки.
5. Отказы `TypeError` по спецификации для `add`/`addAll`/`put`/`match`/`delete`; `AbortSignal` в `add`.
6. `caches` в sandboxed-iframe без `allow-same-origin` бросает `SecurityError`.

## Как проверить

`run_corpus.py --prefixes service-workers/cache-storage --processes 6`: `*.https.any.html` без провалов в атрибутах и без TIMEOUT. Кластер E в заметке пересчитать.
