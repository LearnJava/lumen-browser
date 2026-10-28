# BUG-906 — заголовок ответа `Link: <…>; rel=preload` не даёт ни одного запроса

**Статус:** FIXED 2026-10-28 (P3)
**Заведён:** 2026-08-25 (P1, при починке [BUG-826](BUG-826-FIXED.md) — отделено как остаток)
**Область:** `crates/shell/src/main.rs` (заголовки ответа документа никуда не передаются JS-стороне), `crates/js/src/dom.rs` (`_lumen_link_hints_scan` берёт хинты только с элементов `<link>` в дереве)
**Владелец:** P1/P3

## Симптом

Вторая форма того же хинта, что и элемент `<link rel=preload>` — заголовок
ответа (HTML LS §4.6.6 «Link headers»):

```
HTTP/1.1 200 OK
Link: </preload/resources/dummy.js>; rel=preload; as=script
```

Ни на самом документе, ни на его подресурсе такой заголовок не приводит к
запросу.

## Почему это отдельный баг, а не хвост BUG-826

BUG-826 (починен 2026-08-25) провёл хинт до сети через **JS-шим на самом
элементе** — там же, где живут его `load`/`error`, потому что у шелла нет
по-узлового сигнала завершения, который он мог бы переслать. У заголовка
элемента нет вовсе, поэтому тот же механизм не переиспользуется: нужно либо
довести заголовки ответа документа до JS-стороны (и завести для них
безэлементный список хинтов), либо обрабатывать заголовочную форму в шелле —
но тогда событий слать некому, что для заголовка как раз и правильно.

## Измерение

`tests/wpt/verify_callback_import_preload_gaps.py --variant link-header`
(WPT-RUN-6, срез 27, 2026-08-23): проба отдаёт `Link: <…>; rel=preload;
as=script` и на самом документе, и на подключённом им `.css`, а её http-сервер
не видит запроса ни за одним из двух указанных файлов:

```
[server saw: GET /vcip-linked.css]     ← только сама таблица стилей
lh-rt-entries []
lh-checked
```

Элементная форма на той же странице после починки BUG-826 работает — то есть
проба разделяет две формы, а не измеряет общий отказ.

## Цена

По остатку снимка WPT-RUN-5: `preload/link-header-on-subresource.html`,
`preload/cross-origin-link-header-on-subresource.sub.html`,
`preload/link-header-preload-imagesrcset.html`,
`preload/link-header-modulepreload.html`,
`preload/link-header-preload-non-html.html`,
`preload/link-header-preload-delay-onload.html`.

## Как проверить фикс

`tests/wpt/.venv/bin/python tests/wpt/verify_callback_import_preload_gaps.py
--variant link-header` — в строке `server saw` появляются файлы, названные в
заголовке.

## Починка (2026-10-28, P3)

Выбран второй вариант из «Почему это отдельный баг»: заголовочная форма
обрабатывается в шелле, события слать некому — у заголовка нет элемента.

- `crates/shell/src/page_source.rs`: `emit_response_link_headers` берёт все
  `Link:` итогового ответа документа (оба пути — `load_bytes` и
  `load_bytes_streaming`), общий `emit_link_hints` разбирает их
  `parse_link_header` и гонит через `dispatch_preload_hints` (лог) и
  `warm_preload_cache` (preload/modulepreload/prefetch греют `PREFETCH_CACHE`
  с куками документа и под CSP-заголовком ответа).
- `crates/network/src/lib.rs`: `fetch_subresource_with_links` отдаёт сырые
  значения `Link:` подресурса; `stylesheets.rs`, `scripts.rs` и тёплый прогрев
  `page_load.rs` вызывают `emit_link_hints` внутри замыкания слота кэша — один
  раз на ресурс.
- Замер: `verify_callback_import_preload_gaps.py --variant link-header` —
  сервер видит `GET /vcip-preloaded.js` и `GET /vcip-sub-preloaded.js`,
  `performance.getEntriesByType("resource")` содержит оба.
- Тесты: `response_link_headers_*` (shell), `fetch_subresource_with_links_*` (network).

## Остаток

- `initiatorType` у такой записи `script`, а спека требует `link` для
  `Link`-preload — запись строится по `RequestDestination::Script`.
- Подресурсный `Link:` не проходит через CSP документа (`&[]` в вызовах).
- Подресурсы, кроме CSS/JS из разметки (`site_memory`, `@import`, шрифты), `Link:` не читают.
