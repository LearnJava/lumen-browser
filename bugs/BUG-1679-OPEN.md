# BUG-1679 — `clientX`/`clientY` событий мыши и указателя в окне включают смещение контента (тулбар): курсор в центре элемента даёт `clientY` на 68 px больше

**Статус:** OPEN
**Заведён:** 2026-10-22 (P3, побочная находка при BUG-1458)
**Область:** shell (`crates/shell/src/lumen/pointer.rs` — `js_click_event`, `js_pointer_event`, `js_pointer_move_captured`, `flush_pointer_moves`; вызывающие передают оконные `x_css`/`y_css`)

## Симптом

В окне (не `--no-paint`) события мыши/указателя отдают `clientX`/`clientY` в оконных координатах, а не в координатах вьюпорта страницы (CSSOM View §8: клиентские координаты отсчитываются от вьюпорта). Страница `<div id=h style="width:80px;height:40px">` на `body{margin:8px}`: `h.getBoundingClientRect()` = `[8, 8, 80, 40]`, центр — `(48, 28)`; `test_driver.click(h)` (и `pointerMove` в этот центр) даёт слушателю `click@48,96` — Y больше на `toolbar::CHROME_H` (68). `pointerMove(30, 20)` → `pointermove@30,88`. По X смещения нет (`page_offset().0 == 0`), с левой док-панелью оно появилось бы и там.

## Проба

`tests/wpt`-страница с `testdriver-actions.js` и слушателями `click`/`pointermove`/`mouseover`, запуск `python tests/wpt/run_smoke.py --binary … /css/selectors/<проба>.html`; в слушателе `log.push(e.clientX + "," + e.clientY)`.

| действие | `clientX,clientY` у нас | ожидается |
|---|---|---|
| `pointerMove(30, 20)` | `30,88` | `30,20` |
| `pointerMove(40, 25)` | `40,93` | `40,25` |
| `test_driver.click(h)` (центр `h` — `48,28`) | `48,96` | `48,28` |

## Что делать

Привести координаты к вьюпорту страницы перед `_lumen_dispatch_mouse_event`/`_lumen_dispatch_pointer_event`: вычесть `page_offset()` (не прибавлять скролл — `clientX/Y` от него не зависят). Источник один — вызывающие в `crates/shell/src/lumen/pointer.rs`, `click.rs`, `app/window_event/cursor_moved.rs`; фреймовые ветки (`ft.client`) уже считают клиентскую точку сами. Проверить, что `MouseEvent.pageX/pageY`/`offsetX/offsetY` (шим) выводятся из исправленных `clientX/Y` + скролл, а не добавляют смещение второй раз. Реальный мышиный путь и автоматизация делят эти функции, поэтому правка меняет и то, и другое.

## Как проверить

Таблица выше; тест в `crates/shell` на чистую функцию преобразования окно → вьюпорт, если её вынести.
