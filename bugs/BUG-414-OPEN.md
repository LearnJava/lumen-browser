# BUG-414 — `element.dataset` / `DOMStringMap` отсутствуют (у SVG есть заглушка, возвращающая пустой объект)

**Статус:** OPEN
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
