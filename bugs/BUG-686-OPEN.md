# BUG-686 — `implicit_role` не знает ни одного SVG-тега, `<svg>`/`<circle>`/… откатываются к Generic

**Статус:** OPEN
**Компонент:** a11y (`crates/engine/a11y/src/roles.rs::implicit_role`)
**Найден:** P2, WPT-VENDOR-svg-aam (2026-08-06), прогон не дал сигнала —
находка получена прямой пробой на движке (Rust-тест на `build_ax_tree`), не прогоном

## Симптом

Категория `svg-aam` (`tests/wpt/svg-aam/`, 7 файлов, все `testharness`) целиком
проверяет **implicit**-роли (без `role="..."` атрибута) для `<svg>` и его
графических потомков по спеке SVG-AAM (`https://w3c.github.io/svg-aam/`). Все
7 id тянут общий хелпер `/wai-aria/scripts/aria-utils.js`, который относится к
отдельной ещё не вендоренной категории `wai-aria` (`WPT-VENDOR-wai-aria`,
ROADMAP.md:560) — тот же документированный гэп, что у `core-aam`/`dpub-aam`/
`graphics-aria`/`html-aam` (см. `tests/wpt/VENDOR.md`). `run_report.py --all
--root svg-aam --recursive` отобрал 7 id, все TIMEOUT: манифест не помечает их
`testdriver: true` (тот же класс инверсии предиктора, что `html-aam`/`fledge`),
поэтому вместо мгновенного SKIP страница реально грузится и виснет на
`ReferenceError: AriaUtils is not defined` → **0/7 harness OK, 0/0 сабтестов**.

По правилу «пробуй даже без сигнала» — прямая проба через Rust-тест на
`lumen_a11y::build_ax_tree`:

```rust
let tree = build_tree("<svg><circle cx=\"5\" cy=\"5\" r=\"4\"></circle></svg>");
```

Оба узла (`<svg>` и `<circle>`) попадают в `AXRole::Generic`.

## Причина

`implicit_role` (`roles.rs:349`) — это `match` по локальному имени тега
(`node.element_name()`, строки 355-444), покрывающий HTML-теги (`nav`, `main`,
`button`, `img`, `a`/`area`, `input` через `input_role` и т.д.). Функция ни разу
не проверяет `node.namespace()` и не содержит ни одной ветки для SVG-тегов —
`"svg"`, `"circle"`, `"rect"`, `"path"`, `"g"`, `"text"`, `"image"`, `"a"` (в
SVG namespace) и т.д. просто не совпадают ни с одной веткой и падают в
`_ => AXRole::Generic` (строка 443). Комментарий над функцией (347-348) явно
описывает `Generic` как результат «для элементов без осмысленной семантической
роли (например `<div>`, `<span>`)» — SVG-элементы попадают в эту корзину по
умолчанию, не по замыслу.

Это отдельный от [BUG-398](BUG-398-FIXED.md) пробел: BUG-398 — про explicit
`role="graphics-document"`-атрибут, не распознаваемый `AXRole::parse`; здесь —
про implicit-роль **без всякого `role=`-атрибута**, определяемую спекой
SVG-AAM по одному только тегу (`<svg>` → `graphics-document`/`img`/`group` в
зависимости от контекста вложенности; графические примитивы — `graphics-symbol`
или отсутствие роли, см. таблицу спеки). Раздельные функции (`implicit_role` vs
`AXRole::parse`), раздельные корневые причины, оба гэпа нужно закрывать
независимо.

## Что нужно сделать

1. В `implicit_role` (`roles.rs`) добавить ветку(и) для `svg` — как минимум
   верхнеуровневый `<svg>` → `AXRole::GraphicsDocument` (тот вариант уже нужно
   завести для BUG-398, если оно ещё не сделано на момент фикса — свести оба
   бага в одну реализацию `AXRole::GraphicsDocument`/`GraphicsSymbol`).
2. Проверить `node.namespace()` (или эквивалент), а не только локальное имя
   тега — SVG-теги вроде `a`/`title`/`script` совпадают по имени с HTML-тегами,
   но требуют другой имплицитной роли (или полного её отсутствия) в SVG-контексте.
3. Сверить полную таблицу SVG-AAM (`https://w3c.github.io/svg-aam/#mapping_role_table`)
   для остальных графических примитивов (`circle`/`rect`/`path`/`ellipse`/`line`/
   `polygon`/`polyline`) — большинство из них по спеке не получают отдельной роли
   (остаются без роли / наследуют role="none" от родителя), не `Generic` бездумно.
4. Зависимость на будущее: сами тесты категории (`comp_label.html`,
   `comp_labelledby.html`, `roles.html`, `roles-generic.html`,
   `role-img.tentative.html`) не смогут дать зелёный сигнал, пока не довендорен
   `/wai-aria/scripts/aria-utils.js` (`WPT-VENDOR-wai-aria`, ROADMAP.md:560) —
   чинить implicit-роль можно независимо от этой задачи, verify-петля пока
   только через прямой Rust-тест на `build_ax_tree`.

## Связанные

* `implicit_role` — `crates/engine/a11y/src/roles.rs:349`.
* [BUG-398](BUG-398-FIXED.md) — тот же класс пробела (роль теряется → `Generic`),
  но по explicit `role="..."`-атрибуту, другая функция (`AXRole::parse`).
* [BUG-685](BUG-685-OPEN.md) — соседний, но независимый SVG-гэп: HTML-парсер не
  реализует foreign content, поэтому `<svg>` внутри HTML-документа не получает
  SVG-специфичные JS-прототипы/namespace на уровне DOM/JS. Не проверялось,
  совпадает ли эта же namespace-неосведомлённость с тем, что `implicit_role`
  не смотрит на `namespace()` — возможно общий корень на уровне DOM-слоя,
  стоит перепроверить при фиксе одного из двух багов.
* Категория `WPT-VENDOR-wai-aria` (ROADMAP.md:560) — довендорить
  `/wai-aria/scripts/aria-utils.js`, чтобы вся категория `svg-aam` смогла
  реально исполниться, а не падать TIMEOUT на общем хелпере.
