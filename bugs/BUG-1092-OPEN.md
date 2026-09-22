# BUG-1092 — 13 SVG/SMIL WebIDL-глобалов не заведены вовсе: `SVGAElement`, `SVGAngle`, `SVGNumber`, `SVGNumberList`, `SVGLengthList`, `SVGAnimatedAngle`/`-NumberList`/`-LengthList`, `SVGUnitTypes`, `SVGUseElementShadowRoot`, `ShadowAnimation`, `TimeEvent`, `SVGMPathElement`

**Статус:** OPEN
**Тип:** пробел реализации — глобальные конструкторы SVG DOM не зарегистрированы на `window`.
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 53, `svg`)
**Область:** js — `crates/js/src/svg.rs` (88 присвоений `window.SVG* = ...`, но не для этих 13 имён; `createSVGNumber()`/`createSVGAngle()` на `SVGSVGElement.prototype`, `crates/js/src/svg.rs:522-523`, возвращают голый объект-литерал `{ value: 0, ... }`, а не экземпляр реального класса)
**Владелец:** P3.

## Симптом

`tests/wpt/svg/idlharness.window.html` — единственный самый крупный кластер провала среза (1005 из 3666 `FAIL`-подтестов всего прогона `svg`, 27 %). 145 из них — `assert_own_property: self does not have own property "<Имя>"`, что сверкой по всем сообщениям сводится к ровно 13 уникальным именам:

```
ShadowAnimation
SVGAElement
SVGAngle
SVGAnimatedAngle
SVGAnimatedLengthList
SVGAnimatedNumberList
SVGLengthList
SVGMPathElement
SVGNumber
SVGNumberList
SVGUnitTypes
SVGUseElementShadowRoot
TimeEvent
```

Подтверждено чтением кода: `grep -c 'window\.SVGAElement\|window\.TimeEvent\|...'` по всем 13 именам на `crates/js/src/svg.rs` — 0 совпадений, при 88 других `window.SVG* = ...` присвоениях в том же файле (`SVGRect`, `SVGPoint`, `SVGLength`, `SVGGraphicsElement`, … реально заведены). `SVGAElement` — самый заметный: `<a>` внутри SVG не имеет отдельного класса, `SVG_TAG_MAP` (`crates/js/src/svg.rs:1298-1349`) вообще не содержит ключа `'a'`, элемент падает на дефолтный `SVGElement` (комментарий на `svg.rs:1354-1356` это подтверждает как осознанное поведение — просто неполное). `SVGSVGElement.prototype.createSVGNumber()`/`createSVGAngle()` (`svg.rs:522-523`) возвращают объект-литерал вместо `new SVGNumber(...)`/`new SVGAngle(...)`, поэтому даже фабричные методы не создают типизированный экземпляр.

## Масштаб

- `svg/idlharness.window.html`: 145/1005 `FAIL`-подтестов этого файла — прямое следствие (остальные 860 — другая грань того же файла, см. [BUG-1093](BUG-1093-OPEN.md)).
- Знаковый побочный эффект в отдельном файле: `svg/types/scripted/SVGAnimatedNumber-initial-values.html` (102 подтеста, `Cannot read properties of undefined (reading 'baseVal')`) — весь набор фильтровых примитивов SVG (`feComponentTransfer`, `feConvolveMatrix`, `feDiffuseLighting`, `feDisplacementMap`, `feDistantLight`, `feDropShadow`, `feMorphology`, `fePointLight`, `feSpecularLighting`, `feSpotLight`, `feTurbulence`) тоже отсутствует в `SVG_TAG_MAP` (там заведены только `feBlend`/`feColorMatrix`/`feComposite`/`feGaussianBlur`/`feOffset`/`feMerge`/`feMergeNode` — 7 из 18 `fe*`-элементов спеки), так что и их IDL-атрибуты (`SVGAnimatedNumber` на `k1`/`stdDeviationX`/… ) не существуют — тот же класс дефекта, что и «глобал не заведён», только на уровне элемента, а не значения. Отдельная задача не заводится — чинится тем же проходом, что и `SVG_TAG_MAP`-таблица для этого бага.

## Ожидание

Каждый из 13 классов — реальный конструктор на `window` с прототипом (`SVGNumber`/`SVGAngle`/`SVGNumberList`/`SVGLengthList`/`SVGAnimatedAngle`/`SVGAnimatedNumberList`/`SVGAnimatedLengthList` по образцу уже заведённых `SVGAnimatedLength`/`SVGPointList`; `SVGUnitTypes` — интерфейс с четырьмя константами `SVG_UNIT_TYPE_*`, как уже есть у `SVGPreserveAspectRatio`; `SVGAElement` — `SVG_TAG_MAP['a']`, наследующий `HTMLHyperlinkElementUtils`-подобный набор из `SVGGraphicsElement`; `SVGMPathElement` — `SVG_TAG_MAP['mpath']`; `TimeEvent`/`ShadowAnimation`/`SVGUseElementShadowRoot` — вспомогательные интерфейсы SMIL/`<use>`-shadow, наименьший приоритет). `createSVGNumber()`/`createSVGAngle()` возвращают типизированные экземпляры.

## Связанное

- [BUG-1093](BUG-1093-OPEN.md) — тот же файл, вторая грань: уже заведённые SVG-интерфейсы не соответствуют форме WebIDL (не глобал отсутствует, а прототип не той формы).
- `docs/tasks/p2-test-track.md#test-3-срез-53-2026-09-22`.

## Не проверялось

- Полный список остальных `fe*`-элементов вне `SVGAnimatedNumber-initial-values.html` (их собственные `idlharness`-провалы, если есть, могли попасть в общий пул `svg/idlharness.window.html` под уже учтённые 145/860 — не разделено по элементам).
