# BUG-848 — запрос картинки собирается только с `<img>`: `<video poster>`, `<input type=image>`, SVG `<image>` и `<link rel=icon>` не порождают ни запроса, ни события

**Статус:** OPEN
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером с записью запросов на стороне сервера, есть маркер `element-subresource-never-requested`)
**Область:** `crates/engine/layout/src/box_tree.rs:2262` (`collect_requests_inner` — условие `name.local == "img"` и только оно), `crates/shell/src/main.rs:6858` (`Event::SubresourceHintFound` — единственный потребитель печатает строку в stderr, см. [BUG-826](BUG-826-FIXED.md))
**Владелец:** P1/P3 (`lumen-layout` + шелл). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Ни один из этих элементов не приводит к HTTP-запросу, и ни один не стреляет
`load`/`error`:

```html
<video poster="p.gif" src="v.mp4"></video>
<input type="image" src="p.gif">
<svg><image href="i.svg" width="8" height="8"/></svg>
<link rel="icon" href="icon.gif">
```

Контроль на той же странице — обычный `<img src>` и `<link rel=stylesheet>` —
загружается и (для стиля) стреляет `load`.

## Прямое измерение

`tests/wpt/verify_perf_idb_sse_gaps.py` (2026-08-22, dev-release, Linux,
коммит `bafa603d9`, `--seconds 6`, страницы живы — 11 тиков). Пробный сервер
записывает каждый запрошенный путь, поэтому «запроса не было» доказано
независимо от страницы и независимо от лога браузера (BUG-826: шелл печатает
`⤷ preload …` для запроса, которого не делает):

| вариант | сервер должен был увидеть | увидел |
|---|---|---|
| `req-video-poster` | `/psig-poster.gif` | ничего |
| `req-input-image-src` (разметка + скрипт) | оба `/psig-pixel.gif` | ничего |
| `req-svg-image` | `/psig-image.svg` + контрольный `/psig-pixel.gif` | только контрольный `/psig-pixel.gif` |
| `req-link-icon` | `/psig-icon.gif` + контрольный `/psig-asset.css` | только `/psig-asset.css?control=1` (и `css-load`) |

Событий тоже нет: ни `icon-load`/`icon-error` у `<link rel=icon>`, ни
`input-image-load`/`input-image-error` у `<input type=image>`.

## Причина (локализована чтением кода)

```rust
// crates/engine/layout/src/box_tree.rs:2262
fn collect_requests_inner(doc: &Document, id: NodeId, viewport: Size, out: &mut Vec<ImageRequest>) {
    let node = doc.get(id);
    if let NodeData::Element { name, attrs } = &node.data
        && name.local == "img"
```

Это единственный сборщик запросов картинок из DOM (`collect_image_requests`,
`box_tree.rs:2167`), и он смотрит ровно на один тег. Фоновые картинки идут
отдельным путём (`collect_background_image_requests`), `content: url(...)` —
через layout-дерево; для `poster`, `<input type=image>` и SVG `<image>` пути
нет вообще.

`<link rel=icon>` доходит только до сканера хинтов
(`crates/shell/src/main.rs:6858`), чей единственный потребитель печатает
строку в stderr — то есть повторяет судьбу `preload` из BUG-826.

## Масштаб

Маркер `element-subresource-never-requested` в `tests/wpt/timeout_audit.py` —
**6 id** остатка снимка WPT-RUN-5, всё семейство
`fetch/metadata/generated/element-*`: `element-link-icon`,
`element-video-poster`, `element-input-image`, `svg-image`, плюс медийные
`element-audio`/`element-video`, у которых причина шире —
[BUG-825](BUG-825-FIXED.md)/[BUG-799](BUG-799-FIXED.md). Все они устроены одинаково:
`induceRequest()` навешивает `onload`/`onerror` и ждёт события, после
которого читает заголовки запроса с сервера — событие не приходит, запроса
нет, тест TIMEOUT.

## Направление починки (не предписание)

Расширить `collect_requests_inner` на `poster` у `<video>`, `src` у
`<input type=image>` и `href`/`xlink:href` у SVG `<image>` — все три дают
обычный image request с тем же жизненным циклом, что `<img>`. `<link rel=icon>`
требует потребителя у `SubresourceHintFound` (общая с BUG-826 работа).

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_perf_idb_sse_gaps.py --variant
   req-video-poster --variant req-input-image-src --variant req-svg-image
   --variant req-link-icon` — в `server saw` должны появиться
   `/psig-poster.gif`, оба `/psig-pixel.gif`, `/psig-image.svg`,
   `/psig-icon.gif`.
2. WPT: `run_report.py --all --root fetch/metadata --recursive` — семейство
   `element-*` должно перестать быть TIMEOUT.
