# BUG-848 — запрос картинки собирается только с `<img>`: `<video poster>`, `<input type=image>`, SVG `<image>` и `<link rel=icon>` не порождают ни запроса, ни события

**Статус:** FIXED 2026-08-30 (P3)
**Заведён:** 2026-08-22 (WPT-RUN-6, срез 22 — найден живым замером с записью запросов на стороне сервера, есть маркер `element-subresource-never-requested`)
**Область:** `crates/engine/layout/src/box_tree.rs:2262` (`collect_requests_inner` — условие `name.local == "img"` и только оно), `crates/shell/src/main.rs:6858` (`Event::SubresourceHintFound` — единственный потребитель печатает строку в stderr, см. [BUG-826](BUG-826-FIXED.md))
**Владелец:** P1/P3 (`lumen-layout` + шелл). Заведён P2 в ходе WPT-задачи, здесь не чинится.
**Починка:** `crates/engine/layout/src/box_tree.rs` (`collect_requests_inner`, новая `image_subresource_url`), `crates/js/src/shim/web_api_shim_mid.js` (`_lumen_link_hint_kind`, `_lumen_link_hint_prepare`).

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

## Починка 2026-08-30 (P3)

Два независимых пути, ровно как предполагало «Направление починки».

**`<video poster>` / `<input type=image>` / SVG `<image>` — `collect_requests_inner`.**
Новая функция `image_subresource_url(node: &Node) -> Option<String>` матчит по
`name.local`: `"video"` → атрибут `poster`, `"input"` при
`node.input_type() == Some(InputType::Image)` (типизированное сравнение через
`lumen_dom::InputType`, а не строчное `attr == "image"`, — тот же способ, каким
остальной layout уже читает тип input-а) → атрибут `src`, `"image"` → `href` с
фолбэком на `xlink:href` (тот же фолбэк, каким несколькими строками выше уже
пользуется `<use>`). Ключ для `<video poster>` — сырое значение атрибута, без
резолва базой: та же строка, что `display_list.rs` кладёт в `DrawImage.src` для
poster-кадра, так что fetch-путь и paint-путь используют одну запись
`IMAGE_CACHE` и не расходятся. `resolve_image_source`/picker (`srcset`/`sizes`/
`<picture>`) для этих трёх не годится — они не поддерживают ни один из этих
атрибутов, поэтому запрос собирается напрямую, без пикера.

**`<link rel=icon>` — JS-шим.** Это не `<img>`-подобный узел с DOM-подобным
жизненным циклом, а хинт вставки — тот же класс проблемы, что BUG-826 уже решил
для `preload`/`modulepreload`/`prefetch`, и та же причина держать fetch именно
в шиме на элементе, а не в шелле: `load`/`error` принадлежат элементу, а у
шелла нет по-узлового сигнала завершения, который он мог бы переслать.
`_lumen_link_hint_kind` получил четвёртый токен `icon` — отдельной ветки в
`_lumen_link_hint_prepare` не потребовалось, потому что у иконки, как и у
`prefetch`, нет `as`/`type`-гейтинга (HTML LS §4.6.7 «link type icon» не
объявляет `as`), так что `icon` проваливается в тот же терминальный
`_lumen_link_hint_fetch(nid, href, null)`, что и `prefetch`, и получает
`load`/`error` бесплатно — как и парсерную/скриптовую дедупликацию через
`_lumen_link_hint_done`.

### Замер после починки

Та же проба, все четыре варианта разом (dev-release, Windows, 2026-08-30):

```
req-video-poster      server saw: /psig-poster.gif?poster=1
req-input-image-src   server saw: /psig-pixel.gif?input-script=1, /psig-pixel.gif?input=1
req-svg-image         server saw: /psig-image.svg?svg=1, /psig-pixel.gif?control=1
req-link-icon         markers: css-load, icon-load
                       server saw: /psig-asset.css?control=1, /psig-icon.gif?icon=1
```

Все четыре запроса, которых сервер не видел вовсе, теперь приходят; для
иконки долетают оба маркера (`css-load` — контроль, `icon-load` — предмет).
6 новых юнит-тестов в `box_tree.rs::tests` (`video_poster_produces_an_image_request`,
`video_without_poster_produces_no_request`, `input_type_image_src_produces_an_image_request`,
`input_type_text_with_src_produces_no_request`, `svg_image_href_produces_an_image_request`,
`svg_image_xlink_href_produces_an_image_request`, `svg_image_without_href_produces_no_request`).

### Остаток (не входило в починку)

* **`load`/`error` для `<video poster>`/`<input type=image>`/SVG `<image>` по-прежнему
  не приходят.** «Направление починки» само оговаривало «тот же жизненный цикл,
  что `<img>`» — а у `<img>` сегодня нет диспатча `load`/`error` ни по одному
  пути вставки, это отдельная незакрытая заявка [BUG-630](BUG-630-OPEN.md).
  Замер `req-input-image-src` подтверждает: маркер `input-image-load` не
  появился, хотя оба запроса сервер увидел.
* **SVG `<image>` и `<input type=image>` не рисуются.** У обоих нет потребителя
  в layout/paint (ни `BoxKind`, ни ветки в `display_list.rs`) — байты
  декодируются и попадают в `IMAGE_CACHE`, но на экран не идут. Заявка требовала
  только запроса (сервер видит путь), рисование этих двух элементов в её
  измерении не участвовало и в направлении починки не упоминалось.
* **`<link rel=icon>` не меняет иконку окна/вкладки** — фикс закрывает только
  сетевой запрос и `load`/`error` на самом элементе, никакой связи с UI шелла
  (заголовок окна, favicon в интерфейсе) не заводилось — это никогда не было
  частью симптома.
