# BUG-1569 — CSS Lists: `getComputedStyle` не знает `counter-reset/increment/set`, `list-style-*`; канонические формы (`chapter 1`), недопустимое принимается

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 25, `css/css-typed-om` + `cssom-view` + `cssom` + `css-lists` + `css-counter-styles`)
**Область:** layout/js (`crates/engine/layout/src/selector_query.rs::computed_style_to_map`, разбор `counter-*`/`list-style`)

## Симптом

`getComputedStyle(el).counterReset`, `counterIncrement`, `counterSet`, `listStyleType`, `listStylePosition`, `listStyleImage`, `listStyle` — `""` у всех (10+10+10+27+2+11+5 сабтестов «doesn't seem to be supported in the computed style»); `counter-increment: none chapter`, `counter-reset: none chapter`, `list-style: inside disc outside`, `list-style-image: auto`, `list-style-type: "marker string" none`, `content: counter(foo, none)` принимаются (недопустимы); `counter-increment: chapter` не сериализуется как `chapter 1` (`counter-reset`/`counter-set` — `chapter 0`); `list-style-type: symbols("string")` — 1 из 27 валидных. 17 id, 133 из 181 сабтеста.

## Проба

probe (`--mcp`):

| вызов | у нас | ожидается |
|---|---|---|
| `getComputedStyle(el).counterReset`, `listStyleType`, `listStylePosition` | `""` | `none`, `disc`, `outside` |
| `style.counterIncrement="none chapter"` → чтение | `none chapter` | `""` |
| `style.counterIncrement="chapter"` → чтение | `chapter` | `chapter 1` |
| `style.content="counter(foo, none)"` | принято | отвергнуто |

## Как найдено

WPT-RUN-14 срез 25: `css/css-lists/parsing/{counter-*,list-style*,content-invalid}.html`, `inheritance.html`, `li-counter-increment-computed-style.html`.

## Что делать

Добавить свойства в карту computed, канонизацию и грамматику (`<counter-name> <integer>?`, `none` только отдельно).

## Как проверить

`css/css-lists/parsing/counter-reset-computed.html`, `list-style-type-valid.html`.
