# BUG-463: WAAPI `animate`/`getAnimations` missing from `Element.prototype`

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs:6603-6608` — `_lumen_build_element`)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2` (816 id,
`run_report.py --all --root css/CSS2 --recursive --processes=6`)

## Симптом

```
FAIL Web Animations: property <float> from [initial] to [right] at (-0.3) should be [initial]
  - assert_true: Web Animations should be supported expected true got false
```

428 сабтестов в 7 файлах (`floats/float-no-interpolation.html`,
`floats-clear/clear-no-interpolation.html`, `borders/discrete-no-interpolation.html`,
`tables/border-collapse-no-interpolation.html`, `tables/empty-cells-no-interpolation.html`,
`visufx/animation/visibility-interpolation.html`,
`linebox/animations/line-height-interpolation.html`) — все через общий хелпер
`css/support/interpolation-testcommon.js`.

## Причина

Хелпер feature-detect'ит поддержку WAAPI буквально так
(`interpolation-testcommon.js:178-179`):

```js
name: 'Web Animations',
isSupported: function() {return 'animate' in Element.prototype;},
```

`element.animate()`/`.getAnimations()` в Lumen реально работают (WAAPI Level 1
реализован, `dom.rs:15128` и далее, покрыт юнит-тестом
`element_animate_returns_animation`), но привязаны не к `Element.prototype`, а
как собственные свойства каждого инстанса — `_lumen_build_element` (dom.rs:5596)
создаёт `_obj` как объектный литерал и присваивает `_obj.animate = function(...)`
(dom.rs:6603) прямо на него, ДО того как строка 6746 линкует `_obj`'s прототип на
`HTMLXxxElement.prototype` → … → `Element.prototype`. `'animate' in
Element.prototype` поэтому честно `false`, хотя `'animate' in someElement`
(инстанс) — `true`.

Это расходится с остальным шимом: другие методы (`setAttributeNS`,
`insertAdjacentText`, `getElementsByTagName`, см. BUG-309/299/279) корректно
живут на `Element.prototype`/аналогах.

## Влияние вне WPT

Любой код, feature-detect'ящий WAAPI через `'animate' in Element.prototype`
(частый паттерн в полифиллах и библиотеках анимации) ошибочно считает Lumen
не поддерживающим Web Animations и уходит в CSS-fallback-путь, хотя
функционально API работает.

**WPT-RUN-3 срез 9 (`css/css-backgrounds`, 2026-08-02)** — крупнейшее на
сегодня расширение: 16 файлов / 952 сабтеста, весь `animations/`
поддиректорий категории (`background-color-interpolation.html`,
`background-image-interpolation.html`, `background-position-interpolation.html`,
`background-position-origin-interpolation.html`, `background-size-interpolation.html`,
`border-color-interpolation.html`, `border-radius-interpolation.html`,
`border-width-interpolation.html`, `box-shadow-interpolation.html`,
`discrete-no-interpolation.html` и другие) — тот же хелпер
`interpolation-testcommon.js`'s `'animate' in Element.prototype`
feature-detect, применяется теперь к background/border-свойствам вместо
CSS2's float/clear/table. Три файла (`border-color-interpolation.html`,
`border-radius-interpolation.html`, `border-width-interpolation.html`)
дополнительно частично объяснены [BUG-472](BUG-472-OPEN.md) (18 сабтестов
каждый — `getComputedStyle()` map-гэп на смежных сабтестах того же файла).

## .ini

`tests/wpt/metadata/css/CSS2/{floats/float-no-interpolation,floats-clear/clear-no-interpolation,borders/discrete-no-interpolation,tables/border-collapse-no-interpolation,tables/empty-cells-no-interpolation,visufx/animation/visibility-interpolation,linebox/animations/line-height-interpolation}.html.ini`
— `expected: FAIL` на затронутых сабтестах, флипнуть на `PASS` после переноса
`animate`/`getAnimations` на `Element.prototype`. Срез 9 добавил `.ini` под
`tests/wpt/metadata/css/css-backgrounds/` для тех же 16 файлов.

**WPT-RUN-3 срез 12 (`css/css-logical`, 2026-08-02)** — same
`interpolation-testcommon.js` feature-detect, 6 files/38 subtests:
`animations/float-interpolation.html` (7 of 42 — the rest pass, this file
isn't purely this bug), `animations/margin-block-interpolation.html`,
`animations/margin-inline-interpolation.html`,
`animations/padding-block-interpolation.html`,
`animations/padding-inline-interpolation.html` (6 each), and
`animations/caption-side-no-interpolation.html` (7 — this file initially
showed as a harness-wide TIMEOUT in the first mass run, but a targeted
re-run and the slice's own re-verification pass both show it completing
normally with exactly this bug's symptom; the original TIMEOUT is noted
in its `.ini` as a probable one-off scheduling flake, not a separate
defect). `.ini` added under `tests/wpt/metadata/css/css-logical/` for
these 6 files.
