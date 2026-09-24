# BUG-1118 — картинка, созданная скриптом, запрашивается только после завершения всех скриптов

**Статус:** OPEN
**Заведён:** 2026-09-23 (P2, разбор последовательной загрузки после прогона top100,
[журнал](../docs/perf/journal.md) §2026-09-23 top100 split). Передан P6 по решению пользователя.
**Область:** shell (`crates/shell/src/page_load.rs:1520` `spawn_dynamic_image_loads` — вызывается из
`relayout.rs:1285`, т.е. на relayout после скриптов; headless — этап `fetch-images` в
`page_pipeline.rs:1269` после `run-scripts`). Соседний, но другой дефект — BUG-1048 (у такой
картинки нет `load`/`error`).

## Симптом

Стенд `.tmp/seqlab/` в worktree аудита (`server.py` задерживает каждый ответ на 700 мс и пишет время каждого запроса; `run.py` гонит видимый Chrome 153 и Lumen в окне `--maximized` с холодным HTTP-кэшем). Страница: CSS с `background-image`, 2 sync + async + defer скрипта в `<head>`, 5 `<img>`, скрипт в конце `<body>` с `new Image()` и `fetch('/api')`.

Скрипт `tail.js` делает `new Image(); i.src='/dyn.png'; appendChild(i)` и **затем** `fetch('/api')`.

| | `/api` | `/dyn.png` |
|---|---|---|
| Chrome 153 | 2125 | 2125 — одновременно |
| Lumen, окно | 1641–1922 | 2344–2641 — **после ответа на `/api`** |

Присвоение `src` в скрипте не запускает загрузку: её подхватывает только следующий relayout,
то есть после окончания скрипта (а здесь — ещё и после синхронного `fetch()`, PERF-14).

## Что сделать

HTML LS §4.8.4.3 «update the image data»: присвоение `src`/вставка элемента ставит загрузку
сразу (в параллель), а не ждёт layout. Запускать загрузку из DOM-мутации (как делает streaming-путь
для парсерных `<img>`). Критерий: на стенде `/dyn.png` запрашивается не позже `/api`.

**Срез 1 (2026-09-24, P6):** новый `lumen_core::ext::ImageLoadHook` — нативный `_lumen_set_attr`
(`crates/js/src/v8_runtime/install/dom_core.rs`) синхронно, на своём потоке JS-рантайма, зовёт
шелл-колбэк при присвоении `src` на `<img>` с непустым значением, минуя ожидание relayout.
Шелл (`crates/shell/src/dynamic_image_hook.rs`, `DynamicImgFetchHook`) повторяет тело
`Lumen::spawn_image_requests` для одного запроса — тот же CSP-гейт, тот же `IMAGE_CACHE`/
`decode_image`, тот же дедуп: `Lumen::stream_images_requested` стал `Arc<Mutex<HashSet<String>>>`,
общий с поздним relayout-проходом, так что оба пути не задваивают запрос. Хук собирается и
передаётся в `V8JsRuntime::with_image_load_hook` только в одной точке —
`page_pipeline::parse_and_layout` для рантайма document верхнего уровня
(`app/user_event.rs`'s `LoadDone` → `render_bytes`). **Не покрыто** (везде `None`, узкий срез):
`srcset`/`<picture>` (хук читает только атрибут `src` буквально, без picker'а — сложный выбор
источника остаётся за поздним relayout-проходом), вставка уже готового `<img src>` через
`appendChild`/`innerHTML` без последующего `setAttribute`, iframe-документы (`frames.rs`) и
восстановление вкладки после гибернации (`tab_lifecycle/hibernate.rs`) — там рантайм строится
без хука. Гейт: `scoped-test.sh` зелёный (2098/2098 `lumen-shell`, полный `lumen-js`), `dump_golden.py
--build` 12/12 без дрейфа. Живой прогон стенда `.tmp/seqlab/` не переснят в этом срезе.
