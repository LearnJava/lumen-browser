# BUG-1093 — заведённые SVG-интерфейсы не той WebIDL-формы: члены не на прототипе, геттеры на инстансе не бросают, `enumerable`/`writable` не по спеке

**Статус:** OPEN
**Тип:** пробел реализации — систематическое расхождение формы `svg.rs`-прототипов со спекой WebIDL (Web IDL §3.7 Operations, §3.6 Attributes).
**Заведён:** 2026-09-22 (P2, WPT-RUN-7 срез 53, `svg`)
**Область:** js — `crates/js/src/svg.rs` (классы `SVGElement`, `SVGGraphicsElement`, `SVGGeometryElement`, `SVGSVGElement`, `SVGTextContentElement`, `SVGTransform`, `SVGMarkerElement`, `SVGAnimationElement`, `SVGGradientElement`, `SVGTextPathElement`, `SVGStringList`, `SVGTransformList`, `SVGPointList`, `SVGPatternElement`, `SVGPreserveAspectRatio`, `SVGLength`, `SVGAElement`-семейство и другие — десятки затронутых классов)
**Владелец:** P3.

## Симптом

`tests/wpt/svg/idlharness.window.html`, оставшиеся 860 из 1005 `FAIL`-подтестов файла (после вычета 145, отнесённых к [BUG-1092](BUG-1092-OPEN.md) — полностью отсутствующим глобалам). Разбивка по типу нарушения (взято по тексту сообщения):

| Нарушение | Подтестов | Пример |
|---|---|---|
| Атрибут/операция не найдены на прототипе (`assert_true`/`assert_inherits`) | ~97 | `SVGGraphicsElement interface: attribute requiredExtensions` — `The prototype object must have a property "requiredExtensions" expected true got false` |
| Метод-операция существует, но не `enumerable` (должна быть) | 77 | `SVGGraphicsElement interface: operation getBBox(...)` — `property should be enumerable expected true got false` |
| Геттер атрибута не бросает `TypeError` при вызове на самом прототипе (должен, т.к. это accessor, не own-property инстанса) | 74 | `SVGElement interface: attribute ownerSVGElement` — `getting property on prototype object must throw TypeError` |
| `.length`/аргументы операции не совпадают со спекой (перегрузки/опциональные аргументы) | 57 | `Called with 0 arguments` — операция не бросает при недостатке аргументов |
| Интерфейсный объект сам по себе `enumerable` (не должен быть) | 54 | `SVGGraphicsElement interface: existence and properties of interface object` — `self's property "SVGGraphicsElement" should not be enumerable` |
| `typeof` члена не тот (readonly-атрибут — не `function`/`object`, где ожидается) | 44+34 | `assert_equals: wrong typeof object expected "object" but got "undefined"`, `assert_in_array` |
| Атрибут, который должен быть read-only, — `writable` | 35 | `SVGAnimatedTransformList` и др. — `property should not be writable expected false got true` |
| Константы (`SVG_LENGTHTYPE_*`, `SVG_MARKER_ORIENT_*`, `TEXTPATH_*`, …) отсутствуют как own-property интерфейсного объекта | ~27 | `expected property "SVG_PRESERVEASPECTRATIO_..."` |

Все эти паттерны — один и тот же корень: `svg.rs`'s классы объявлены обычным ES-синтаксисом (`class X { get foo() {...} }`), где часть членов (судя по сообщениям про «не enumerable» и «не бросает на прототипе») на самом деле навешана на **инстанс** конструктором (`this.foo = ...`/`this.getBBox = function(){...}`), а не на `X.prototype` через геттер/метод — тот же класс дефекта, что уже фиксировался для других подсистем ([BUG-677](BUG-677-OPEN.md) `shape-detection`, [BUG-1087](BUG-1087-OPEN.md) `trusted-types`, дубликат [BUG-544](BUG-544-DUPLICATE.md) `Element.prototype.animate`). Разница с уже пофикшенными — здесь это касается **всей SVG-иерархии** (`SVGElement` как общий базовый класс всех остальных), поэтому дефект наследуется каждым потомком: `SVGGraphicsElement`, `SVGGeometryElement`, `SVGTextContentElement`, `SVGSVGElement`, `SVGAElement`, `SVGMarkerElement`, `SVGGradientElement`, `SVGTextPathElement`, `SVGAnimationElement`, `SVGPatternElement` и списочные типы (`SVGStringList`/`SVGTransformList`/`SVGPointList`/`SVGNumberList`) — 20+ классов в аггрегате по имени интерфейса (`SVGTextContentElement` 51, `SVGGraphicsElement` 48, `SVGPreserveAspectRatio` 47, `SVGLength` 46, `SVGSVGElement` 43, `SVGAElement` 42, `SVGTransform` 39, `SVGMarkerElement` 39, `SVGElement` 38, `SVGAngle` 33, `SVGAnimationElement` 30, `SVGNumberList` 28…).

Повторяющиеся отсутствующие члены — не разная логика на каждый интерфейс, а один и тот же наследуемый набор: `className` (наследуется от `Element`, но `svg.rs`'s `SVGElement` его не проксирует), `ownerSVGElement`, `viewportElement`, `requiredExtensions`, `systemLanguage` (`SVGTests`-mixin — не примешан вовсе, 22+22 подтеста `assert_inherits`), `pathLength` на `SVGGeometryElement`.

## Ожидание

Каждый затронутый метод/геттер перенесён на `X.prototype` через настоящий `Object.defineProperty(..., {enumerable, configurable, get})`/`{value, enumerable: <по спеке>, writable: false}` вместо присвоения на `this`; `SVGTests`-mixin (`requiredExtensions`/`systemLanguage`/`className` для `SVGElement`/`SVGGraphicsElement`) примешивается один раз в общий класс. Интерфейсные объекты сами получают `enumerable: false` (как остальные уже заведённые в WEB_API_SHIM конструкторы).

## Связанное

- [BUG-1092](BUG-1092-OPEN.md) — тот же файл, первая грань (глобалы отсутствуют полностью, а не просто не той формы).
- [BUG-677](BUG-677-OPEN.md), [BUG-1087](BUG-1087-OPEN.md), дубликат [BUG-544](BUG-544-DUPLICATE.md) — тот же класс дефекта в других подсистемах, для справки по паттерну фикса.
- `docs/tasks/p2-test-track.md#test-3-срез-53-2026-09-22`.

## Не проверялось

- Точное сопоставление «какой конкретно член на каком конкретно классе» — таблица выше агрегирует по типу нарушения и по имени интерфейса, не по паре (интерфейс, член); экономически дешевле поднимать при самом фиксе, читая `svg.rs` для каждого класса напрямую, чем вручную расписывать все ~80 членов здесь.
