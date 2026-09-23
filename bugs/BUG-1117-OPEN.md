# BUG-1117 — фоновые картинки CSS запрашиваются только после layout: всегда последняя волна

**Статус:** OPEN
**Заведён:** 2026-09-23 (P2, разбор последовательной загрузки после прогона top100,
[журнал](../docs/perf/journal.md) §2026-09-23 top100 split). Передан P6 по решению пользователя.
**Область:** shell (`crates/shell/src/page_pipeline.rs:1579` — `fetch-bg-images` после `layout_page`,
который сам ждёт `fetch-images` ради intrinsic-размеров; `subresources.rs:68`
`fetch_and_decode_background_images(&layout, …)` собирает URL из дерева layout).

## Симптом

Стенд `.tmp/seqlab/` в worktree аудита (`server.py` задерживает каждый ответ на 700 мс и пишет время каждого запроса; `run.py` гонит видимый Chrome 153 и Lumen в окне `--maximized` с холодным HTTP-кэшем). Страница: CSS с `background-image`, 2 sync + async + defer скрипта в `<head>`, 5 `<img>`, скрипт в конце `<body>` с `new Image()` и `fetch('/api')`.

| | Запрос `bg.png`, мс от запроса документа |
|---|---|
| Chrome 153 | 2125 (вместе со второй волной картинок) |
| Lumen, окно | 3063–3344 — **последний** запрос страницы, после `<img>`, `fetch()` и картинки из скрипта |
| Lumen, headless `--trace-nav` | 3843 (`fetch-bg-images` стартует после третьего `layout`) |

## Корень

URL фона берутся из готового дерева layout, а layout ждёт загрузки всех `<img>`. Фон размеры
боксов не меняет (комментарий в `page_pipeline.rs` это прямо говорит), значит ждать layout ему
не нужно: достаточно каскада.

## Что сделать

Собирать `background-image: url()` после каскада (computed style), а не после layout, и запускать
их загрузку вместе с `<img>`. Критерий: на стенде `bg.png` запрашивается в одной волне с `<img>`.
