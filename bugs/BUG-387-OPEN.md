# BUG-387 — `computedStyleMap()` читает только инлайн-атрибут `style`, а не каскад — Typed OM возвращает `undefined` для любого CSS-свойства, заданного через `<style>`/таблицы стилей

**Статус:** OPEN
**Компонент:** js (`crates/js/src/typed_om_api.rs:76-89` — `StylePropertyMap.prototype.get`;
`crates/js/src/v8_runtime.rs:3658` и `crates/js/src/dom.rs:3105` — нативная
привязка `_lumen_get_style_property`)
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

## Связанные

* [BUG-382](BUG-382-OPEN.md) — `getComputedStyle()`/`getBoundingClientRect()`
  гонка (пустые в ~75% загрузок); тот же сериализатор `computed_style_to_map`
  нужен как источник для фикса здесь.
