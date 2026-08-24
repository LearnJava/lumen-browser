# BUG-822 — событие `scrollend` не диспатчится ниоткуда: ни у страницы, ни у элемента, и `onscrollend` на `window` нет

**Статус:** FIXED 2026-08-24 (P1)
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

Важно, что это **не** [BUG-821](BUG-821-FIXED.md): элемент реально
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
[BUG-821](BUG-821-FIXED.md): точка, где считается «позиция изменилась с
прошлого кадра», нужна обоим.

## Как проверить фикс

1. `tests/wpt/.venv/bin/python tests/wpt/verify_stream_scroll_message_gaps.py
   --variant scroll-element-scrollto --variant scroll-window` — печатает
   `el-scrollend` / `win-scrollend` и `onscrollend-in-window=true`.
2. WPT: `run_report.py --all --root css/css-scroll-snap --recursive` —
   подкаталог `snap-after-relayout` перестаёт висеть.

## Починено (P1, 2026-08-24)

Заведена пара к существующим `_lumen_fire_scroll_*`:
`_lumen_fire_scrollend_on_element(nid)` / `_lumen_fire_window_scrollend_event()`
(`crates/js/src/dom.rs`, рядом со `scroll`-парой), Rust-обёртки
`V8JsRuntime::fire_element_scrollend` / `fire_window_scrollend` и методы
`PersistentJs` в шелле. Событие, как и `scroll`, non-bubbling и
non-cancelable.

**Ключевое решение — где считается «скролл закончился».** Спека (CSSOM-View
§14) требует одного `scrollend` на *последовательность*, а не на движение,
поэтому одного «сдвинулось ли» из [BUG-821](BUG-821-FIXED.md) мало. Долг
живёт в рантайме: `V8JsRuntime::page_scrollend_due(moved, settled)` рядом с
`page_scroll_y` и по той же причине — он **подокументный**, иначе долг,
взятый на уходящей странице, был бы выплачен на пришедшей. Шелл на шаге 1
`RedrawRequested` считает `settled` = «ничто больше не двигает страницу»
(`scroll_anim.is_none() && momentum_anim.is_none() && scroll_drag.is_none()`)
и передаёт его вместе с `moved`:

* инстант-скролл (`window.scrollTo`, find-in-page, клавиша с мгновенным
  прыжком) — `moved && settled`, оба события в одном кадре, что §14 прямо
  разрешает;
* smooth-анимация и инерция — `scroll` каждый кадр, долг копится, выплата на
  том кадре, где анимация обнулилась: `advance_scroll_anim`/`advance_momentum`
  сбрасывают себя ровно на том кадре, где последний раз двигают страницу;
* последний кадр инерции может упереться в край и не сдвинуть ничего —
  `moved=false, settled=true` с накопленным долгом всё равно даёт `scrollend`.

Элементный путь проще: обе точки записи скролла контейнера
(программный дренаж `take_scroll_requests` и колесо через
`try_scroll_overflow_container`) применяют позицию мгновенно, отдельной
анимации у контейнера нет — значит, последовательность закончилась в том же
кадре, и `fire_element_scrollend` идёт сразу за `fire_element_scroll`.

Отдельно закрыт feature-detect: `onscroll: null, onscrollend: null` добавлены
в литерал `window`. Диспатч-сторона правки не потребовала —
`window.dispatchEvent` в общей ветке и так читает `window['on' + type]`
(BUG-392), не работал только голый `in`.

**Замеры (dev-release, Windows, `--seconds 6`):**

| проба | было | стало |
|---|---|---|
| `scroll-window` | `onscrollend-in-window=false`, нет `win-scrollend` | `onscrollend-in-window=true`, `win-scroll y=300` + `win-scrollend y=300`, затем `win-scroll y=400` + `win-scrollend y=400` |
| `scroll-element-scrollto` | `el-scroll top=250`, `el-scrollend` нет | `el-scroll top=250`, `el-scrollend top=250` |
| `scroll-element-scrolltop` | `el-scroll top=400`, `el-scrollend` нет | `el-scroll top=400`, `el-scrollend top=400` |
| smooth `scrollTo({behavior:'smooth'})` (разовая проба) | — | `scroll=16`, `scrollend=1` — ровно один, в конце |

Тесты: `page_scrollend_is_due_on_an_instant_scroll`,
`page_scrollend_waits_for_the_animation_to_settle`,
`page_scrollend_is_paid_even_if_the_last_frame_does_not_move`,
`fire_window_scrollend_reaches_a_window_listener`,
`fire_element_scrollend_reaches_an_element_listener`,
`onscrollend_is_detectable_on_window`,
`window_onscrollend_handler_is_invoked` (`lumen-js`).

## Остаток (не чинилось здесь)

* **Жест тачпада без инерции.** Флага «палец на тачпаде» в шелле нет
  (`touchpad_vel` обнуляется и на `Started`, и на `Ended`), а страничный
  wheel/тачпад идёт через `scroll_by_smooth`, т.е. через анимацию, которая
  между двумя событиями движения успевает завершиться. Пауза посреди жеста
  поэтому закрывает одну последовательность и открывает следующую — вместо
  одного `scrollend` на жест их будет несколько. Для WPT это неважно (там
  программный скролл), для страницы — косметика.
* **Колесо над контейнером** даёт `scrollend` на каждый щелчок: у контейнера
  нет анимации, каждый щелчок — законченная мгновенная последовательность.
  Настоящие браузеры здесь дебаунсят по таймеру; такого механизма в движке
  нет, а альтернатива («не слать вовсе») оставила бы пользовательский скролл
  контейнера без завершающего события совсем.
* **Горизонталь.** `window.scrollX` по-прежнему захардкожен в 0
  ([BUG-821](BUG-821-FIXED.md)), так что горизонтальное движение страницы не
  даёт ни `scroll`, ни `scrollend`.
* **`document.onscrollend = fn`** не вызывается — это общий
  [BUG-874](BUG-874-OPEN.md) (`document.dispatchEvent` читает только реестр
  слушателей); на `window` обе формы работают.
* Путь `scroll_container_into_view` (обход предков BUG-338) меняет позицию
  контейнера, не диспатчя **ни** `scroll`, ни `scrollend` — предсуществующая
  дыра, шире этого бага.
