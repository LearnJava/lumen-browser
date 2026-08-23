# BUG-897 — конструируемых таблиц стилей нет (`new CSSStyleSheet()` — `ReferenceError`), а `adoptedStyleSheets` — инертное expando: присваивание принимает что угодно, включая `[null]`, и не делает ничего

**Статус:** OPEN
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `cssom-constructed`)
**Область:** js (`crates/js/src/dom.rs` — ни `CSSStyleSheet`, ни аксессора `adoptedStyleSheets` на `document`/теневом корне; свойство создаётся самим присваиванием как обычное поле объекта)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Вторая половина CSSOM — конструируемая. Читающая половина уже описана в
[BUG-471](BUG-471-OPEN.md)/[BUG-746](BUG-746-OPEN.md) (`document.styleSheets`,
`<style>.sheet`, классы правил); здесь — записывающая:

* `new CSSStyleSheet()` — `ReferenceError: CSSStyleSheet is not defined`,
  значит `replaceSync`/`replace` недостижимы;
* `document.adoptedStyleSheets` до присваивания — `undefined` (по спецификации
  пустой `FrozenArray`), то есть штатная проверка «а поддерживается ли»
  проходит как «не поддерживается»;
* присваивание `document.adoptedStyleSheets = [x]` **не бросает ничего** при
  любом содержимом массива (в замере — `[null]`, потому что конструктор
  недоступен) и после этого честно читается как массив длины 1. Никакого
  эффекта на стили нет: это обычное свойство объекта. Ровно то же на теневом
  корне — и там оно ещё и теряется на следующем чтении `host.shadowRoot`,
  потому что обёртка каждый раз новая ([BUG-877](BUG-877-OPEN.md)).

Ловушка для любой пробы и для любого сайта: «присвоил — прочитал — совпало»
здесь ничего не доказывает.

## Прямое измерение

`tests/wpt/verify_cssom_svg_interface_gaps.py --variant cssom-constructed`
(2026-08-23, dev-release, Linux):

```
new-CSSStyleSheet THREW CSSStyleSheet is not defined
replaceSync       THREW Cannot read properties of null (reading 'replaceSync')
doc-adopted = undefined        doc-adopted-set = 1
attachShadow = object
shadow-adopted = undefined     shadow-adopted-set = 1
```

## Цена по WPT

`shadow-dom/declarative/tentative/shadowrootadoptedstylesheets/shadowrootadoptedstylesheets-fetched-module.html`
(остаток снимка WPT-RUN-5) и всё семейство `css/cssom/CSSStyleSheet-*` /
`shadowrootadoptedstylesheets-*`; кроме того, это блокирует CSS-модули
([BUG-896](BUG-896-OPEN.md)), которым нечего экспортировать.

## Что дальше

Одной работой с [BUG-746](BUG-746-OPEN.md): объект таблицы стилей в JS-слое
плюс плюмбинг «разобранный `Stylesheet` из шелла → рантайм». Конструируемая
таблица проще читающей — её содержимое приходит из `replaceSync`, а не из
шелла, — но применять её всё равно должен каскад, так что точка подключения
общая.
