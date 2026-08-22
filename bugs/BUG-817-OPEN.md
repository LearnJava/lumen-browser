# BUG-817 — событие `scrollend` не диспатчится ниоткуда: ни у страницы, ни у элемента, и `onscrollend` на `window` нет

**Статус:** OPEN
**Заведён:** 2026-08-21 (WPT-RUN-6, срез 19 — 7 TIMEOUT остатка, механизм `scrollend-never-fired`)
**Область:** `crates/js/src/dom.rs:15881-15892` (есть `_lumen_fire_scroll_on_element` и `_lumen_fire_window_scroll_event`, парного `scrollend` нет), `crates/shell/src/main.rs:3496-3510` (обе Rust-обёртки, `scrollend` отсутствует)
**Владелец:** P1/P3 (шелл + `lumen-js`). Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Тест скроллит контейнер и ждёт конца скролла — ждёт вечно:

```js
// css/css-scroll-snap/snap-after-relayout/resnap-on-overflowing-snap-area.html
const scrollendPromise = new Promise(resolve => {
  scroller.addEventListener("scrollend", resolve);
});
scroller.scrollTo(0, expectedPosition);
await scrollendPromise;   // ← сюда управление не возвращается
```

Важно, что это **не** [BUG-816](BUG-816-OPEN.md): элемент реально
скроллится и событие `scroll` получает. Не хватает только завершающего
`scrollend`.

## Прямое измерение

`tests/wpt/verify_stream_scroll_message_gaps.py` (2026-08-21, коммит
`6e60c8aa8`, `--seconds 6`):

| проба | получено |
|---|---|
| `scroll-window` | `onscrollend-in-window=false`; ни `win-scrollend` |
| `scroll-element-scrollto` | `el-scroll top=250` есть, `el-scrollend` — нет |
| `scroll-element-scrolltop` | `el-scroll top=400` есть, `el-scrollend` — нет |

То есть отсутствует и сам факт доставки, и feature-detect: `'onscrollend' in
window` даёт `false`, хотя `onscrollend` числится в списке имён
обработчиков `dom.rs:995`.

## Причина (локализована чтением кода)

Механизма нет вовсе. Грep по воркспейсу даёт `_lumen_fire_scroll_on_element`
и `_lumen_fire_window_scroll_event` (`dom.rs:15881`, `:15887`) с
Rust-обёртками (`main.rs:3496`, `:3505`) — и ни одной функции, ни одного
вызова для `scrollend`. Соответственно ни один путь скролла (колесо,
клавиатура, программный, инерция, smooth-анимация) не может его
диспатчить.

Отдельно: `'onscrollend' in window` ложно, потому что список `dom.rs:995`
описывает *обработчики body-элемента* и не создаёт свойства на `window`.
Для WPT это скорее плюс — тест, который проверяет наличие свойства, честно
падает вместо того чтобы зависнуть, — но для страничного feature-detect
это несогласованность с тем, что имя в списке присутствует.

## Масштаб

Механизм `scrollend-never-fired` забирает **7 id** остатка снимка
WPT-RUN-5 — все из `css/css-scroll-snap/snap-after-relayout` (7 из 8
непонятых TIMEOUT подкаталога). Оценка снизу: `scrollend` — штатный способ
дождаться конца smooth-скролла, и его ждут также `css/css-scrollend`,
`css/css-scroll-snap/scroll-*`, часть `uievents`; в снимке те до своих
ожиданий не дошли по более старшим причинам.

Вне WPT: страница, ожидающая конца скролла перед дорогой работой
(дозагрузка, перерисовка карты), на Lumen эту работу не начнёт никогда —
типовой обходной путь на `setTimeout` после `scroll` у неё, скорее всего,
есть, но сам событийный контракт не выполняется.

## Направление починки (не предписание)

Завести пару к существующим: `_lumen_fire_scrollend_on_element(nid)` /
`_lumen_fire_window_scrollend_event()` в `dom.rs` рядом с
`_lumen_fire_scroll_*`, обёртки в `main.rs`, и звать их там, где скролл
*закончился*: по завершении `scroll_anim`/инерции, а для инстант-путей — на
том же кадре, что и `scroll` (спека допускает оба события в одном кадре,
если скролл был мгновенным). Полезно делать это одной правкой с
[BUG-816](BUG-816-OPEN.md): точка, где считается «позиция изменилась с
прошлого кадра», нужна обоим.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_stream_scroll_message_gaps.py
   --variant scroll-element-scrollto --variant scroll-window` — печатает
   `el-scrollend` / `win-scrollend` и `onscrollend-in-window=true`.
2. WPT: `run_report.py --all --root css/css-scroll-snap --recursive` —
   подкаталог `snap-after-relayout` перестаёт висеть.
