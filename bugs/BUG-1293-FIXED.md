# BUG-1293 — CSS-переходы и анимации стартуют на следующем кадре: синхронное чтение `getComputedStyle()`/`getAnimations()` видит конечное значение

**Статус:** FIXED 2026-10-19 (P3)
**Заведён:** 2026-10-06 (P2, WPT-RUN-14 срез 5, `css/css-transforms`)
**Область:** layout/shell (`TransitionScheduler`/`AnimationScheduler` создают переход/анимацию при тике кадра, а не при пересчёте стиля); связано с [BUG-536](BUG-536-FIXED.md), [GAP-CSSANIM](../ROADMAP.md)

## Симптом

`--dump-layout`, один скрипт, без ожидания кадра — ровно как делает `interpolation-testcommon.js`:

```js
d.style.opacity = '0'; getComputedStyle(d).opacity;           // 0
d.style.transition = 'opacity 100s -50s linear';
d.style.opacity = '1';
getComputedStyle(d).opacity;   // 1, ожидается 0.5
d.getAnimations().length;      // 0, ожидается 1
```

То же для `transform` (`matrix(1,0,0,1,100,0)` вместо `…,50,0)`), `margin-left`, `perspective-origin`, `color` (`rgb(100,100,100)`
вместо 50) — любое значение сразу конечное. Через `@keyframes` с `animation-delay: -50s` — `none`/начальное значение.
Для **Web Animations** тот же сценарий верен: `el.animate([...]).pause(); currentTime = 25` даёт `0.25`/`matrix(…,25,0)`,
`composite: 'add'` тоже верен — то есть интерполятор исправен, не хватает момента старта.

Что происходит после кадра, headless-проба не показывает (`--dump-layout` не крутит кадры и таймеры) — не проверялось.

## Как найдено

WPT-RUN-14 срез 5: `css/css-transforms/animation/*` — 15 файлов, 1681 упавший сабтест (`transform-interpolation-00{1…6}`,
`perspective-origin-interpolation`, `list-interpolation`, `transform-interpolation-computed-value`, …) в режимах
`CSS Transitions`, `CSS Transitions with transition: all`, `CSS Animations`; режим `Web Animations` падает заметно реже
(260 из 2526). Из них 1681 — крупнейший кластер модуля.

## Что делать

Создавать переход при смене computed-значения (CSS Transitions L1 §3 — «style change event», момент — пересчёт стиля, который
`getComputedStyle()` и `getAnimations()` обязаны форсировать), а анимацию из `animation-name` — при том же пересчёте;
значение на момент времени 0 + `delay`. `GAP-CSSANIM` закрыт, но его срезы добавляли публикацию значения в снэпшот раз в кадр.

## Как проверить

`css/css-transforms/animation/transform-interpolation-00{1…7}.html`, `list-interpolation.html`; общий харнесс
`css/support/interpolation-testcommon.js` подключают 337 файлов `css/` (`css-transitions`, `css-animations`, `css-values`…).

## Дополнение (WPT-RUN-14 срез 6, `css/css-backgrounds/animations/*`)

Тот же механизм виден на reftest'ах: `background-color-animation.html`, `background-color-transition.html` и ещё 27 (`animations/background-color-*`, `two-background-color-animation-diff-length*`) — `thick`. `animation: bgc 1000000s cubic-bezier(0,1,1,0) -500000s` на `<div>` с `background-color:green`: `--dump-display-list` отдаёт `FillRect … #008000ff` (значение не из ключевых кадров, а из стиля элемента), `document.getAnimations().length` = 0 в той же задаче; тест ждёт `getAnimations()[0].ready` и снимает экран. Режимы `CSS Animations`/`CSS Transitions`/`CSS Transitions with transition: all` у `background-*`/`border-*` в этом срезе — 1 280 упавших сабтестов; часть из них — [BUG-1305](BUG-1305-FIXED.md) (неявный кадр, шорткоды, `background-size`/`box-shadow`) и `animation-composition` (`CSS-SPECS.md`).


## Повторное измерение: WPT-RUN-14 срез 24 (2026-10-08)

Тот же механизм виден в reftest с `animation-play-state: paused` и отрицательной задержкой (`css-filters-animation-*`, `css-backdrop-filters-animation-*` — 20 id): `@keyframes{opacity:0→1}` с `animation: a 4s linear -2s paused` в `--screenshot` рисуется с `opacity:1` (конец), `getComputedStyle().opacity` — `1`; для `0s paused` — тоже `1` (начало не применено).

## Повторное измерение: WPT-RUN-14 срез 26 (2026-10-09)

Старт перехода нестабилен; условие не изолировано. `run_smoke.py` + testharness, `transition: opacity 5s linear; opacity: 0`, затем `style.opacity = "1"`, замер `getAnimations().length` и `getComputedStyle().opacity` через 200–300 мс (в спецификации к этому моменту значение ≈ 0.05):

| условие | `getAnimations()` | `opacity` | прогонов |
|---|---|---|---|
| изменение в том же такте, что создание элемента, без чтения стиля и с чтением (`flush`) | 0 | 1 | 2 из 2 (спецификация: без чтения перехода нет, с чтением — есть) |
| элемент создан скриптом 60–500 мс назад | 1 | 0.04–0.13 | 8 из 8 |
| разметочные элементы, изменение через 0,5 с после старта теста | 1 | 0.12 / 0.17 | 2 из 2 |
| разметочные элементы, изменение через 1,5–1,9 с после старта теста | 1 | **1** | 8 из 8 |
| 6 разметочных элементов, изменения в 0,1 / 0,4 / 0,9 / 1,6 / 2,4 / 3,3 с | 1 | **1** | 6 из 6 |
| 6 элементов (3 разметочных + 3 созданных скриптом 0,7 с назад), изменение без чтения / с чтением стиля | 1 | **1** | 12 из 12 (два прогона по 6) |
| изменение в обработчике `load` | 1 | 0.068 | 2 из 2 |
| изменение в скрипте до `load` | 0 | 1 | 2 из 2 |

То есть кроме «`getAnimations()` пуст сразу» (основная запись) есть второй симптом: объект перехода создан, а значение уже конечное. Первый переход на странице через `el.style.cssText += ';transition:background-color 0.2s;background-color:#fff'` не присылает `transitionrun`/`transitionend` (`css-transitions/crashtests/clear-duration-in-transitionend.html` — TIMEOUT), а тот же приём на втором–четвёртом элементе той же страницы присылает (2 прогона по 4 элемента, 1 из 4 без событий в каждом). Влияет на `css-transitions/changing-while-transition-*`, `starting-of-transitions-001`, `shadow-root-insertion`, `transitions-retarget` (`Transition should be initially N% complete`), `events-001…006`.

## Исправление (P3, 2026-10-19)

**Корень.** Rust-планировщики (`TransitionScheduler::sync` в `relayout`, `AnimationScheduler::tick` в кадре) создают переход/анимацию на следующем кадре, а `getComputedStyle()`/`getAnimations()` читают снимок, который до того момента хранит установившееся значение. Единственный синхронный наблюдатель — само чтение, поэтому ту же схему, что у `*-rule-*` (GAP-DECOR), обобщили на все свойства: `web_api_shim_tail_b.js::_wa_motion_read`. Первое чтение запоминает установившееся (before-change) значение; чтение, нашедшее другое, запускает переход, если `transition-*` элемента его покрывают (`_wa_motion_cover`: `all`, longhand и его шорткоды); `animation-name` вычисляет `@keyframes` на тех же часах (`performance.now()`, один на задачу). `getAnimations()` пересчитывает переходы и регистрирует тени `Animation` (с таймингом, `finished`, `startTime`, отмена по `AbortError`).

**Что обязательно для остальных читателей снимка** (ловушки — в `subsystems/js.md`, последняя запись):
- смена обнаруживается по *computed*-значению (`_lumen_get_computed_value`; для `transform` — список функций `TRANSFORM_LIST_KEY` рядом с resolved `matrix()`, иначе `rotate(720deg)` и `translateY(70%)` теряются);
- свойства, которые ведёт кадровый цикл (`opacity`/`transform`/цвета/`height`), оверлей не трогает, пока планировщик пишет их в снимок (`animated_marker_key`, `_lumen_computed_is_animated`) — иначе значение планировщика принимается за смену стиля и переход перезапускается на каждом чтении;
- свойство, которого нет в снимке (`rotate`/`scale`/`translate`/`transform-origin` — BUG-1435), не анимируется;
- нерисуемый элемент (`display: none`) переходов не запускает.

**Интерполяция.** `transform` — нативно (`crates/engine/layout/src/transform_interp.rs`, Transforms L2 §15: общий префикс функций одного семейства, единичные функции вместо недостающих, 3D-unmatrix + slerp кватернионов, вырожденная пара → перелом); остальное — в JS (`_wa_motion_try`: цвета в premultiplied sRGB, списки длин по компонентам, целые свойства округляются, диапазоны `opacity`/размеров зажимаются, `display` видим весь прогон, `animation-composition: add|accumulate` в ключевых кадрах).

**Измерено** (`run_report.py`, ту же сборку без правки как база):

| Категория | До | После |
|---|---|---|
| `css/css-transforms/animation` | 2819 / 4926 | 4256 / 4926 |
| `css/css-transitions` | 379 / 3298 | 619 / 3298 |
| `css/css-animations` | 489 / 1403 | 580 / 1403 |
| `css/css-backgrounds/animations` | 1482 / 5129 | 1807 / 5129 |
| `css/css-color` | 5179 / 11338 | 5228 / 11338 |
| `css/cssom`, `web-animations` | 2063 / 4002, 1938 / 4381 | 2063, 1939 |

**Остаток** (в записи не закрыт, вынесен): `!important` в таблице стилей должен перебивать `@keyframes`, и четыре теста перешли OK → TIMEOUT — [BUG-1648](BUG-1648-OPEN.md); классы `CSSAnimation`/`CSSTransition` — [BUG-1581](BUG-1581-OPEN.md); `rotate`/`scale`/`translate` в снимке — [BUG-1435](BUG-1435-FIXED.md); `em` внутри `calc()` в `translate*()` при интерполяции берёт 16px (а не `font-size` элемента) — в `transform_interp.rs::interpolate_transform_value`; 3D-пары, где Rust-парсер `parse_transform_list` не знает функцию, не интерполируются.

**Тесты:** `crates/js/src/dom/tests/v8_bug1293_motion_overlay.rs` (13), `transform_interp::tests` (10).
