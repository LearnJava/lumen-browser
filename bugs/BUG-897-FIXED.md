# BUG-897 — конструируемых таблиц стилей нет (`new CSSStyleSheet()` — `ReferenceError`), а `adoptedStyleSheets` — инертное expando: присваивание принимает что угодно, включая `[null]`, и не делает ничего

**Статус:** FIXED 2026-09-06 (P1, [CSSOM-5](../ROADMAP.md))
**Тип:** нереализованная функциональность, не дефект реализованного кода — велась как задача `CSSOM-5` в [ROADMAP.md](../ROADMAP.md). Переклассифицировано 2026-09-02 ре-триажем пула WPT-RUN-5/6. Строка перенесена из BUGS.md в BUGS-FIXED.md 2026-09-22 при закрытии [GAP-CSSMOD](../ROADMAP.md) (BUG-896) — CSSOM-5 сама закрылась ещё 2026-09-06, но BUGS.md не был обновлён на закрытии задачи.
**Заведён:** 2026-08-23 (WPT-RUN-6, срез 29 — живой замер, вариант `cssom-constructed`)
**Область:** js (`crates/js/src/dom.rs` — ни `CSSStyleSheet`, ни аксессора `adoptedStyleSheets` на `document`/теневом корне; свойство создаётся самим присваиванием как обычное поле объекта)
**Владелец:** P1/P3. Заведён P2 в ходе WPT-задачи, здесь не чинится.

## Симптом

Вторая половина CSSOM — конструируемая. Читающая половина уже описана в
[BUG-471](BUG-471-FIXED.md)/[BUG-746](BUG-746-FIXED.md) (`document.styleSheets`,
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
  потому что обёртка каждый раз новая ([BUG-877](BUG-877-FIXED.md)).

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
([BUG-896](BUG-896-FIXED.md)), которым нечего экспортировать.

## Что дальше

Одной работой с [BUG-746](BUG-746-FIXED.md): объект таблицы стилей в JS-слое
плюс плюмбинг «разобранный `Stylesheet` из шелла → рантайм». Конструируемая
таблица проще читающей — её содержимое приходит из `replaceSync`, а не из
шелла, — но применять её всё равно должен каскад, так что точка подключения
общая.

## Исправлено

`CSSOM-5` (ROADMAP.md), срезы 1-3, закрыта 2026-09-06:

* **Срез 1:** `new CSSStyleSheet()`/`.replaceSync()`/`.replace()`/`.cssRules`
  реализованы поверх отдельного реестра `ConstructedStylesheets`
  (`crates/js/src/v8_runtime/install/constructed_stylesheets.rs`) — у
  сконструированного листа нет владеющего DOM-узла, поэтому он не может
  делить индексное пространство со `stylesheet_nodes` (CSSOM-1).
  `document.adoptedStyleSheets`/`shadowRoot.adoptedStyleSheets` стали
  настоящим валидируемым списком: присваивание не-`CSSStyleSheet`/не-
  сконструированного элемента бросает `TypeError`.
* **Срез 2:** `document.adoptedStyleSheets` подключён к каскаду —
  `document_adopted_fingerprint`/`document_adopted_stylesheet`
  (`constructed_stylesheets.rs`) дают отпечаток и мёрдж, `crates/shell/src/
  relayout.rs::refresh_dynamic_css` перестраивает лист только при изменении.
* **Срез 3 (закрывает задачу):** `CSSStyleSheet.insertRule`/`.deleteRule` на
  сконструированном листе (`Stylesheet::insert_rule`/`delete_rule`,
  `crates/engine/css-parser/src/parser.rs`).

Тени по-прежнему вне скоупа — у Lumen нет теневого каскада вообще.
Разблокировало [BUG-896](BUG-896-FIXED.md) (GAP-CSSMOD).
