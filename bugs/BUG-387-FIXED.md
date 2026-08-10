# BUG-387 — `computedStyleMap()` читает только инлайн-атрибут `style`, а не каскад — Typed OM возвращает `undefined` для любого CSS-свойства, заданного через `<style>`/таблицы стилей

**Статус:** FIXED 2026-08-10 (P3)
**Компонент:** js (`crates/js/src/typed_om_api.rs` — `TYPED_OM_SHIM` целиком;
`crates/js/src/v8_runtime.rs` — привязки Typed OM; `crates/js/src/dom.rs` —
`computedStyleMap()` в обёртке элемента)
**Найден:** P2, WPT-VENDOR-forced-colors-mode (2026-07-28), прогон
`run_report.py --root forced-colors-mode` (тест `forced-colors-mode-40.html`)

## Симптом

```html
<style>div { color: green; }</style>
<div id="div">…</div>
<script>
  document.getElementById("div").computedStyleMap().get("color")   // → undefined
  getComputedStyle(document.getElementById("div")).color           // → "rgb(0, 128, 0)" (верно)
</script>
```

`computedStyleMap().get(prop)` отвечает `undefined` на любое свойство, заданное
не инлайн-атрибутом `style="…"`, а обычным правилом таблицы стилей — то есть
почти на все реальные страницы. Тест `forced-colors-mode-40.html` проверяет
именно это (`element.computedStyleMap().get(property).toString()` для 17
CSS-свойств, заданных через `<style>`) — все 17 сабтестов падают одинаково:
`Cannot read properties of undefined (reading 'toString')`.

## Причина

`element.attributeStyleMap` (мутируемый `StylePropertyMap`, по спеке отражает
**только** инлайн `style=""`) и `element.computedStyleMap()` (по спеке —
**резолвленное/используемое** значение с учётом каскада, аналог
`getComputedStyle`) в Lumen — один и тот же класс с одним и тем же геттером:

```js
// crates/js/src/typed_om_api.rs:80
StylePropertyMap.prototype.get = function(prop) {
  var val = _lumen_get_style_property(this.__nid__, String(prop));
  ...
};
// ComputedStylePropertyMap.prototype = Object.create(StylePropertyMap.prototype);
```

а сама нативная привязка `_lumen_get_style_property` (`v8_runtime.rs:3658`)
читает исключительно атрибут `style` узла и парсит его как строку:

```rust
if let Some(style_attr) = node.get_attr("style") {
    let parsed = _parse_style_string(style_attr);
    ...
}
String::new()   // атрибута style нет вовсе → пустая строка → JS undefined
```

Каскад/computed style (тот же движок, что стоит за `getComputedStyle`, которая
работает верно) не задействован вовсе. Поэтому `computedStyleMap()` фактически
тождественна `attributeStyleMap` — при инлайн-стиле обе дают одинаковый (верный)
ответ, а без него `computedStyleMap()` молча теряет всё, что видит
`getComputedStyle`.

## Как чинить

`ComputedStylePropertyMap.prototype.get` должен резолвить значение через тот же
путь, что `getComputedStyle` (сериализация `computed_style_to_map`, уже
используемая для `getComputedStyle`/BUG-382), а не через `_parse_style_string`
инлайн-атрибута. Разделить нативную привязку на две: одну для
`attributeStyleMap` (текущее поведение — инлайн-атрибут, корректно для мутируемой
карты) и новую для `computedStyleMap()` (полный computed style).

Регрессия проверяется без WPT: `<style>div{color:green}</style>`, затем
`document.getElementById('div').computedStyleMap().get('color').toString()`
должно дать `"rgb(0, 128, 0)"`, а не `undefined`.

## Как починено (2026-08-10, P3)

Не заплатка поверх геттера, а разворот иерархии в спековую (Typed OM L1 §6):
базовый **`StylePropertyMapReadOnly`** (это и есть то, что возвращает
`computedStyleMap()`), мутируемый `StylePropertyMap` **расширяет** его.
Источник чтения выбирает флаг `__computed__` на прототипе подкласса, а не
вызывающий, поэтому «унаследовать не тот читатель» больше нечем: computed-карта
ходит в те же снимки каскада, что `getComputedStyle`
(`_lumen_get_computed_style` + `_lumen_get_custom_property` — второй потому,
что кастомные свойства живут в отдельной наследуемой карте, [BUG-732](BUG-732-FIXED.md)),
инлайн-карта — по-прежнему в атрибут `style=""`. Имени
`ComputedStylePropertyMap` (его нет в спеке ни у кого) больше не существует;
`StylePropertyMap`/`StylePropertyMapReadOnly` выставлены на `window` как
настоящие интерфейсы.

Попутно в том же пути чтения:

* **Перечисление было мёртвым — и мёртвым с исключением.**
  `_lumen_get_style_entries` возвращал литерал `"[]"` (строку), а шим звал
  `.entries()` на ней. Теперь у каждой карты свой источник:
  `_lumen_get_style_entries` — инлайн-декларации, новый
  `_lumen_get_computed_style_entries` — резолвленный каскад вместе с
  кастомными свойствами; оба JSON-ом, отсортированным по имени (обе стороны —
  `HashMap`, иначе порядок обхода одной страницы плавал бы между прогонами).
  На этом источнике сделаны `size`/`entries`/`keys`/`values`/`forEach`/
  `@@iterator`/`getAll` по форме `iterable<USVString, sequence<CSSStyleValue>>`.
* **Имя кастомного свойства регистрозависимо**, а ключ лукапа гнался через
  `_camel_to_kebab`: `--Foo` → `---foo`, декларация теряется. Добавлен
  `_css_property_key`, пропускающий `--`-имена как есть — сразу во всех четырёх
  привязках, иначе `set('--Foo')` и `get('--Foo')` разошлись бы.
* **Значение цвета/списка заворачивалось в `CSSKeywordValue`**, то есть
  объявлялось одиночным CSS-идентификатором, которым не является. Теперь по
  форме строки: размерность → `CSSUnitValue` (спековые имена единиц
  `number`/`percent`; знак и дробная часть, которых старая регулярка не
  принимала), голый идентификатор → `CSSKeywordValue`, остальное → базовый
  `CSSStyleValue`.
* **`CSSUnitValue.to()` переклеивал ярлык единицы, не пересчитывая число** —
  тихо неверное значение. Считает по группам абсолютных единиц
  (длина/угол/время/частота/разрешение) и бросает `TypeError` там, где пересчёт
  без контекста разрешения не определён (`px`→`em`), вместо ответа наугад.

**Гейт:** 14 тестов `dom::tests::v8_bug387_computed_style_map`. Первый —
симптом заявки дословно; второй важнее — он проверяет, что у двух карт
*разные* читатели: у фикстурного `#main` нет инлайн-стиля, поэтому мутируемая
карта обязана быть пустой ровно там, где computed отдаёт значение каскада.
Живая проба через `--mcp-live-port` на `<style>#d{color:green;font-size:21px;--gap:8px}</style>`:
`computedStyleMap().get('color').toString()` = `rgb(0, 128, 0)`,
`get('fontSize')` = `21px`, `get('--gap')` = `8px`, `size` = 81,
`attributeStyleMap.get('color')` = `undefined`.

**Остаток, который это не чинит.** Тест-заявка `forced-colors-mode-40.html`
идёт с 0/17 на 8/17. Оставшиеся 9 свойств (`caret-color`,
`column-rule-color`, SVG-краски `fill`/`stroke`/`flood-color`/`lighting-color`/
`stop-color`, `-webkit-tap-highlight-color`, `-webkit-text-emphasis-color`)
есть в `ComputedStyle`, но отсутствуют в списке `computed_style_to_map` —
это [BUG-472](BUG-472-OPEN.md), общий гэп карты computed style, одинаково
бьющий и по `getComputedStyle`. Отдельный баг не заведён: дубликат.

Форма WebIDL интерфейсов (конструктор со страницы должен бросать
`TypeError`, атрибуты — геттеры прототипа вместо собственных данных) не
трогалась: в заявке её не было, и это класс [BUG-366](BUG-366-FIXED.md), а не
путь чтения.

Второе ограничение — общее с `getComputedStyle` и по построению: computed-карта
читает **снимок**, заполняемый проходом layout, а не резолвер по запросу
([BUG-472](BUG-472-OPEN.md)). Скрипт, выполняющийся синхронно на этапе парсинга,
видит `undefined` у `computedStyleMap()` ровно тогда же, когда
`getComputedStyle()` отвечает `""` — проверено пробой `--dump-layout` на той же
странице; после `load` оба отвечают верно (проба `--mcp-live-port` выше). Это не
остаток BUG-387: обе карты и `getComputedStyle` теперь читают один и тот же
источник, так что расхождения между ними больше нет ни в какой момент.

## Связанные

* [BUG-472](BUG-472-OPEN.md) — гэп покрытия `computed_style_to_map` и
  снимочная (не по запросу) природа computed style; ограничивает сверху то, что
  может отдать computed-карта.
* [BUG-382](BUG-382-FIXED.md) — доставка снимка layout/computed-style к моменту
  загрузки; тот же снимок, что читает эта карта.
* [BUG-732](BUG-732-FIXED.md) — отдельный наследуемый снимок кастомных свойств,
  второй источник computed-карты.
