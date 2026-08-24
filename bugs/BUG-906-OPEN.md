# BUG-906 — заголовок ответа `Link: <…>; rel=preload` не даёт ни одного запроса

**Статус:** OPEN
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
