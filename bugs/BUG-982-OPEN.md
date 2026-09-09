# BUG-982: `innerHTML`/`outerHTML`/`insertAdjacentHTML` lose the leading
whitespace run of a fragment

**Статус:** OPEN
**Дата:** 2026-09-04
**Компонент:** js (`crates/js/src/v8_runtime/dom_helpers.rs::parse_html_fragment`)
/ engine (`crates/engine/html-parser`)
**Найден:** P3, 2026-09-04, при разборе устаревшей ветки, закрывавшей BUG-413
(измерение застало этот баг как побочный шум); подтверждён повторным чтением
кода на актуальном `main`

## Механизм

`parse_html_fragment` parses the fragment with the *document* parser, not a
context-element-aware fragment parser:

```rust
// crates/js/src/v8_runtime/dom_helpers.rs
/// ... HTML LS §13.4 fragment-context tree-construction adjustments are not
/// implemented, matching the existing `Foreign content is not supported` gap
/// noted in `tree_builder.rs`, BUG-685).
pub(super) fn parse_html_fragment(doc: &mut lumen_dom::Document, html: &str) -> Vec<lumen_dom::NodeId> {
    let temp = lumen_html_parser::parse(html);
    let root = temp.body().unwrap_or_else(|| temp.root());
    ...
}
```

`lumen_html_parser::parse` starts in the document parser's `initial`
insertion mode. HTML LS §13.2.6.4.1–4 (`initial`/`before html`/`before
head`/`in head`) require whitespace-only character tokens to be **ignored**
while in those modes — that's what lets a real document's leading
`\n  <html>` not create a stray text node. The real fragment-parsing
algorithm (§13.4) is supposed to skip straight to an insertion mode picked
from the *context element* (`in body` for a `<div>` context, which is what
every `innerHTML`/`outerHTML`/`insertAdjacentHTML` call effectively uses) —
that mode does not swallow whitespace. Because Lumen's fragment parser is
just the document parser aimed at a scratch document, the leading
whitespace run of the fragment string is consumed by those early
whitespace-eating modes before the parser ever reaches `in body`.

Observable effect:

```js
d.innerHTML = ' abc';   // textContent === 'abc' — leading space gone
d.innerHTML = ' ';      // no text node created at all
d.innerHTML = 'abc ';   // fine — trailing space survives
d.innerHTML = 'a  b';   // fine — interior whitespace survives
d.innerHTML = '<b> x</b>'; // fine — whitespace inside an element survives
```

Only the very start of the fragment is affected; everything downstream of
the first non-whitespace token parses normally.

## Related

[BUG-685](BUG-685-OPEN.md) is the sibling symptom of the same root cause
(`parse_html_fragment` has no real context-element handling at all): there
it's namespace/foreign-content (SVG/MathML markup landing in the HTML
namespace), here it's insertion-mode/whitespace. A proper §13.4
implementation — pick the initial insertion mode from the context element's
tag/namespace instead of always starting at `initial` — would close both.

## Цена

Measured while chasing BUG-413's `innerText` getter: 43 of the 316 subtests
in `innertext-with-white-spaces.html` fail only because the test harness
builds its DOM via `innerHTML` (all 316 pass when the same test markup is
built via `createTextNode` instead — confirms the getter itself is not at
fault). Affects any code path that goes through `parse_html_fragment`:
`innerHTML` setter, `outerHTML` setter, `insertAdjacentHTML`.

---

## Расширение 2026-09-09 (P6, дорожка E2E): теряются не только пробелы

Тот же механизм отбрасывает **comment-узлы**:

```js
d.innerHTML = '<!--$-->x';
d.textContent   // 'x' — комментария в дереве нет
```

Комментарий в начале фрагмента — это comment-токен в режиме `initial` /
`before html`, а HTML LS §13.2.6.4.1 предписывает вставлять его в **сам
Document**, не в `<body>`. `parse_html_fragment` забирает только детей
`temp.body()`, поэтому узел остаётся в выброшенном временном документе.
Симптом другой, причина та же, что у ведущих пробелов: фрагмент разбирается
документным парсером вместо §13.4.

Важно, что сами comment-узлы реализованы и переносятся между документами —
ломается именно эта выборка:

- `NodeData::Comment` — `crates/engine/dom/src/lib.rs:222`;
- парсер их создаёт — `crates/engine/html-parser/src/tree_builder.rs`
  (6 мест вызова `create_comment`);
- `import_node` их копирует — `crates/js/src/v8_runtime/dom_helpers.rs:358`.

## Цена выше, чем измерено

React 18 расставляет `<!--$-->` как маркеры границ Suspense и читает у них
`.data`. Поэтому баг блокирует гидрацию любого приложения на Next.js App
Router: живая проба против внешнего стенда (2026-09-09, после локального
снятия [BUG-599](BUG-599-OPEN.md)) даёт
`TypeError: Cannot read properties of null (reading 'data')`.
До этого баг стоил 43 сабтеста про whitespace — теперь он ещё и второй из
двух блокеров E2E-пригодности движка.

**Узкая правка и полная — разные задачи.** Забрать узлы вне `<body>` дешевле
и закрывает оба симптома; полноценный §13.4 fragment parsing algorithm —
доработка, она числится за [BUG-685](BUG-685-OPEN.md)
(`OPEN (ДОРАБОТКА → GAP-XMLDOC)`).
