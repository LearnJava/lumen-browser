# BUG-414 — `element.dataset` / `DOMStringMap` отсутствуют (у SVG есть заглушка, возвращающая пустой объект)

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs` — фабрика живых обёрток `_lumen_build_element`,
`:5516-5880`; заглушка — `crates/js/src/svg.rs:265`)
**Найден:** 2026-07-28 (P2), WPT-VENDOR-html, срез `html/dom`

## Симптом

Проба (`--dump-layout`, `<p id=t1 data-foo="bar" data-multi-word="x">`):

```
el.dataset=undefined | dataset.foo=n/a | DOMStringMap=undefined
```

`data-*`-атрибуты при этом читаются штатно через `getAttribute('data-foo')` — отсутствует
именно `dataset`-проекция и её интерфейс.

Единственная реализация в кодовой базе — заглушка у SVG-элементов:

```js
// crates/js/src/svg.rs:265
get dataset() { return {}; }
```

Она хуже, чем отсутствие: `svgEl.dataset.foo` не бросает, а молча отдаёт `undefined`,
и каждое обращение возвращает **новый** объект (`svgEl.dataset !== svgEl.dataset`), то есть
запись в него теряется без ошибки — классический тихий отказ.

## Что требует спека

HTML LS §3.2.6.6 «The `dataset` IDL attribute» + WebIDL `DOMStringMap`:

- `element.dataset` — живая проекция всех атрибутов с префиксом `data-`;
- преобразование имён: `data-multi-word` ↔ `dataset.multiWord` (kebab → camel и обратно;
  имя с заглавной после дефиса — `SyntaxError`);
- поддержка `in`, `delete`, `for…in` (именованные свойства), присваивание создаёт/меняет
  атрибут;
- глобальный `DOMStringMap` должен существовать как интерфейс;
- `[Exposed=Window]` на `HTMLElement`, а также на `SVGElement` и `MathMLElement`.

## Данные WPT

Срез `html/dom` (`run_report.py --all --root html/dom --recursive`),
`elements/global-attributes/`:

| Файл | Сабтесты | Сообщения |
|---|---|---|
| `dataset.html` | 0/7 | `DOMStringMap is not defined` (3 — HTML/SVG/MathML), `Cannot use 'in' operator to search for 'foo' in undefined` (4) |
| `dataset-binding.window.html` | 0/2 | `DOMStringMap is not defined` |

Показательно, что сабтест «SVG elements should have a `.dataset`» падает **тем же**
`DOMStringMap is not defined`, а не проходит благодаря заглушке из `svg.rs` — тест проверяет
`instanceof DOMStringMap`, и пустой объектный литерал его не удовлетворяет.

## Направление починки

`Proxy` поверх существующих `getAttribute`/`setAttribute`/`removeAttribute` обёртки
(`has`/`get`/`set`/`deleteProperty`/`ownKeys`) + глобальный `DOMStringMap` как прототип
результата — тот же приём, которым уже сделаны живые `HTMLCollection` в
[BUG-310](BUG-310-FIXED.md). Заглушку `svg.rs:265` при этом **удалить**, а не оставить рядом:
две реализации одного имени — ровно та ситуация, из-за которой SVG-сабтест сейчас падает не
по своей причине.

## Срез 33 (`css/css-sizing`, 2026-08-03)

4 files (`contain-intrinsic-size/{auto-007,contain-intrinsic-size-{009,032,033}}.html`)
TIMEOUT via this bug, root-caused live (`--mcp-live-port`, `resource://console`):
`test.dataset.expectedClientHeight = ...` throws `Cannot set properties of
undefined (setting 'expectedClientHeight')` before any `test()`/`checkLayout()`
registers, so wptrunner waits out the full timeout with zero subtests.
Confirmed via a fresh-element probe that `dataset` is `undefined` on every
tag (`div`/`select`/`button`/`input`/`canvas`/…), not select-specific — this
bug's blast radius is any WPT file that uses `element.dataset` inside a
synchronous top-level `<script>` before test registration, independent of
element type. `.ini` under `tests/wpt/metadata/css/css-sizing/`, file-level
`expected: TIMEOUT`.

## Исправлено (P3, 2026-08-09, в ходе разбора [BUG-703](BUG-703-FIXED.md))

`dataset` реализован ровно предложенным здесь способом: `Proxy` поверх
`_lumen_get_attr`/`_lumen_set_attr`/`_lumen_remove_attr`/`_lumen_get_attr_names`
с ловушками `get`/`set`/`has`/`deleteProperty`/`ownKeys`/
`getOwnPropertyDescriptor` (`crates/js/src/dom.rs`, `_lumen_make_dataset`),
преобразованием `data-multi-word` ↔ `multiWord` и `SyntaxError` на имени вида
`-x`. Целью прокси служит `Object.create(DOMStringMap.prototype)`, поэтому
`el.dataset instanceof DOMStringMap` истинно; сам `DOMStringMap` заведён как
неконструируемый интерфейсный объект (`new DOMStringMap()` → `TypeError`).
Обёртка кэшируется на элементе, так что `el.dataset === el.dataset`.

Заглушка `svg.rs:265` удалена, как и предписывал этот баг: реальные SVG-узлы
приходят из нативного `createElementNS` и несут собственный живой геттер
общей обёртки, поэтому прототипная заглушка только затеняла его для
конструируемых вручную экземпляров.

Найдено заново на живой странице: `script.dataset.mmid = ...` в бутстрапе
`tbank.ru` падал с `TypeError: Cannot set properties of undefined`.

Тесты: `dataset_maps_data_attributes_both_ways`,
`dataset_is_a_domstringmap_on_html_and_svg` в `dom::tests::v8_core`.
Сабтесты WPT (`elements/global-attributes/dataset.html`,
`dataset-binding.window.html`) заново не измерялись — прогон категории `html`
в этой сессии не запускался.
