# BUG-1117 — фоновые картинки CSS запрашиваются только после layout: всегда последняя волна

**Статус:** FIXED 2026-09-24 (P6)
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

## Решение (2026-09-24, P6)

URL фона теперь считаются по каскаду, без layout:
`lumen_layout::collect_cascade_background_image_requests(doc, sheet, viewport, dark, dpr)`
(`crates/engine/layout/src/box_tree/image_requests.rs`) обходит DOM с тем же каскадом, что
строит дерево боксов, и возвращает тот же набор, что `collect_background_image_requests(&layout)`:
`background-image` всех слоёв (включая `image-set()`/`cross-fade()`), `list-style-image` у
`display: list-item`, фон и `content: url()` у `::before`/`::after`. Поддеревья без боксов
(`display: none`, закрытый popover, SVG `<defs>`) пропускаются, у `display: contents`
потомки остаются.

В `parse_and_layout` (`crates/shell/src/page_pipeline.rs`, спан `prefetch-bg-images`) сразу
после скриптов и до `fetch-iframes`/`fetch-images` поток `lumen-bg-urls` (стек
`DEEP_TREE_STACK_BYTES`) считает эти URL по снимку документа и таблицы стилей, а
`subresources::spawn_background_image_prefetch` загружает их в `IMAGE_CACHE` через тот же гейт
CSP `img-src`. Послелейаутный `fetch_and_decode_background_images` остаётся авторитетным
списком, но идёт через `decode_background_image` → `IMAGE_CACHE.get_or_decode`: URL, который
уже загружен или загружается, берётся из кэша (или дожидается своего слота), второго запроса
нет. Слот заполняет общий `decode_image`, поэтому URL, который одновременно фон и анимированный
`<img>`, не теряет анимацию; фону достаётся первый кадр, как и раньше. Печать в PDF
(`media_print`) не предзагружается — её каскад идёт с media `print`.

Стенд `.tmp/seqlab` (700 мс на ответ):

| | `bg.png`, мс | `<img>` (первая), мс | последний ответ, мс |
|---|---|---|---|
| Lumen, окно, до | 3063–3344 (последний запрос) | — | — |
| Lumen, окно, после (два прогона) | 1750 / 2516 | 1593 / 2360 | 2453 / 3219 |
| Lumen, headless, после | 3078 | 3078 | 3781 |

Абсолютные времена гуляют от прогона к прогону (джиттер TLS через VPN), но разрыв между первой `<img>` и `bg.png` — 156 мс в обоих прогонах, одна волна. `bg.png` запрашивается ровно один раз. Критерий «одна волна с `<img>`» выполнен.

Тесты: `layout_generation_misc::cascade_bg_urls_match_layout_collector` (сверка с
послелейаутным сборщиком на наборе разметок), `cascade_bg_urls_skip_boxless_elements`.
Гейты: `cargo clippy --workspace --all-targets -D warnings` чист; `scripts/scoped-test.sh` —
один FAILED, `frame_bridge::tests::inaccessible_bridge_mutation_does_not_mark_dirty` в
`lumen-js` (крейт не затронут): известный флак [BUG-1110](BUG-1110-OPEN.md), в отдельном
прогоне проходит. Пиксели не двигаются: для не-GIF картинок декод тот же (`decode_to`), а GIF
в фоне в корпусе `graphic_tests` нет.
