# BUG-816 — программный скролл страницы не диспатчит событие `scroll`: `fire_window_scroll` зовётся только из обработчика колеса мыши

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 19 — 9 TIMEOUT остатка, механизм `page-scroll-no-scroll-event`)
**Область:** `crates/shell/src/main.rs:16085` (единственный вызов `fire_window_scroll`, ветка `MouseScrollDelta`), `crates/shell/src/main.rs:14222` (слив `take_page_scroll_requests` → `scroll_to`/`start_smooth_scroll`), `crates/shell/src/main.rs:3505` + `crates/js/src/dom.rs:15887` (`_lumen_fire_window_scroll_event`), `crates/js/src/dom.rs:4213` (`Element.scrollIntoView` — вторая грань)
**Владелец:** P1/P3 (шелл + `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

`window.scrollTo(0, 300)` **скроллит** страницу — `window.scrollY` после
этого честно отдаёт 300 — но события `scroll` не получает никто:

```js
// css/css-scroll-anchoring/adjustment-followed-by-scrollBy.html, сокращённо
center.scrollIntoView({ block: "center", behavior: "instant" });
await new Promise(resolve => window.addEventListener("scroll", resolve));
// ← дальше этой строки тест не проходит никогда
```

Элементный скролл при этом работает полностью: и `el.scrollTo(0, 250)`, и
`el.scrollTop = 400` двигают контейнер и дают `scroll` на элементе. Ломается
ровно страничный путь.

## Прямое измерение

`tests/wpt/verify_stream_scroll_message_gaps.py` (2026-08-21, коммит
`6e60c8aa8`, `--seconds 6`, все пробы живы — по 11 тиков):

| проба | получено |
|---|---|
| `scroll-window` | `after-scrollTo y=300`, `after-scrollBy y=400` — и ни одного `win-scroll` |
| `scroll-window-smooth` | `after-smooth y=300` — и ни одного `win-scroll` |
| `scroll-element-scrollto` | `el-scroll top=250`, `after-scrollTo top=250` — событие есть |
| `scroll-element-scrolltop` | `el-scroll top=400`, `after-assign top=400` — событие есть |
| `scroll-intoview` | `after-intoView y=0` — страница даже не сдвинулась |

Позиция читается по таймеру, а не сразу после вызова: запрос кладётся в
очередь для шелла и применяется на следующем кадре. Ловушка при
воспроизведении: распорка пробной страницы должна что-то **рисовать** —
`content_height` шелла считается по display-list-у, поэтому у пустого
`<div style="height:4000px">` `max_scroll()` равен нулю и страница
действительно не скроллится (первый замер этого среза так и ошибся).

## Причина (локализована чтением кода)

JS-путь целый: `window.scrollTo` (`dom.rs:11363`) → `_lumen_request_page_scroll`
(`v8_runtime.rs:3797`) → `about_to_wait` сливает очередь (`main.rs:14222`) →
`scroll_to` / `start_smooth_scroll` (`main.rs:22785`, `:22800`) двигают
`scroll_y`, а `set_page_scroll_y` на следующем кадре (`main.rs:16218`)
возвращает значение в JS. Чего в этой цепочке нет — вызова
`fire_window_scroll`: у него ровно один call site на весь воркспейс,
`main.rs:16085`, в ветке `WindowEvent::MouseWheel`. То есть событие `scroll`
привязано не к изменению позиции, а к конкретному устройству ввода.

Симметричный элементный путь сделан правильно: `fire_element_scroll`
вызывается там, где применяется `pending_scrolls`, поэтому `scrollTo`/
`scrollTop=` на контейнере событие дают.

**Вторая грань, того же семейства:** `Element.scrollIntoView`
(`dom.rs:4213`) поднимается по предкам до ближайшего элемента со
скролл-состоянием и, если такого нет, **не делает ничего** — фоллбэка на
страничный скролл нет. Поэтому `scrollIntoView` на обычном элементе в потоке
документа не двигает страницу вообще (проба `scroll-intoview`: `y=0`), а
тесты якорения, которые с него начинаются, не доходят даже до первого
ожидания. Аргументы `scrollIntoView` при этом игнорируются целиком — это
уже описано в [BUG-479](BUG-479-OPEN.md); здесь речь о том, что метод не
скроллит вовсе.

## Масштаб

Механизм `page-scroll-no-scroll-event` забирает **9 id** остатка снимка
WPT-RUN-5 — 9 из 10 непонятых TIMEOUT категории `css/css-scroll-anchoring`
(десятый, `reading-scroll-forces-anchoring.html`, ждёт не события, а
синхронного эффекта чтения `scrollY`, и остаётся в остатке честно).

Оценка снизу по двум причинам: категория `css/css-scroll-anchoring` в снимке
почти целиком не дошла до своих ожиданий, и любой тест вне неё, который
ждёт `scroll` после программного скролла, до сих пор списан на более
старшие механизмы. Вне WPT цена та же: страница, подписанная на
`window.onscroll` (ленивая подгрузка, sticky-хедеры, инфинити-скролл),
на Lumen не увидит ни одного программного скролла — включая скролл,
который сама же и запросила.

## Направление починки (не предписание)

Перенести вызов `fire_window_scroll` туда, где меняется `scroll_y` —
в `scroll_to` / `start_smooth_scroll` / шаг анимации (`advance_scroll_anim`)
и в клавиатурный/тач-путь, — вместо ветки колеса мыши. По спеке
(CSSOM-View §14) событие ставится в очередь на шаге «run the scroll steps»
для каждого скроллера, чья позиция изменилась с прошлого кадра, поэтому
естественная точка — тот же кадровый шаг, где уже зовётся
`set_page_scroll_y` (`main.rs:16218`): сравнить с прошлым значением и
диспатчить при отличии. Это заодно снимает дублирование события при
колесе. Отдельным (меньшим) шагом — фоллбэк `scrollIntoView` на страничный
скролл, когда скроллящегося предка нет.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_stream_scroll_message_gaps.py
   --variant scroll-window --variant scroll-intoview` — печатает `win-scroll`
   после каждого шага и `after-intoView y > 0`.
2. WPT: `run_report.py --all --root css/css-scroll-anchoring --recursive` —
   семейство перестаёт висеть.
