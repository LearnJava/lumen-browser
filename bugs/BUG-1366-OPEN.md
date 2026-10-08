# BUG-1366 — Однодвоеточные `:before` / `:after` / `:first-line` / `:first-letter` не распознаются как псевдоэлементы (CSS 2.1 §5.12.3)

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 14, `css/CSS2` (text, linebox, fonts, generated-content, lists, bidi-text))
**Область:** css-parser (`crates/engine/css-parser/src/parser/selectors.rs::parse_pseudo` — `is_element` истинно только при `::`; имена `before`/`after`/`first-line`/`first-letter` ищутся лишь в ветке `is_element`)

## Симптом

CSS 2.1 §5.12.3 и CSS Pseudo-Elements L4 §2.1: четыре унаследованных псевдоэлемента допустимо писать с одним двоеточием, и браузеры это поддерживают (в реальных стилях `a:before`, `p:first-letter` — почти везде). `lumen --dump-display-list`, одна страница, `p` с `content`:

| селектор | результат |
|---|---|
| `.a:before{content:"A1 "}` | `a` — `::before` не создан |
| `.b::before{content:"B2 "}` | `B2 b` — верно |
| `.c:after{content:" C3"}` | `c` — `::after` не создан |
| `p:first-letter{color:red;font-size:30px}` | `Hello` одним `DrawText` — буквица не выделена |
| `p::first-letter{…}` | `H` + `ello` — верно |

`parse_pseudo` (`selectors.rs:1367`) принимает `:before` как `PseudoClass::Unsupported("before")` (ветка `lower.as_str()` для классов, `_ => Unsupported`), и правило не совпадает ни с чем.

## Как найдено

WPT-RUN-14 срез 14: из 820 упавших reftest `CSS2/{text,linebox,fonts,generated-content,lists,bidi-text}` 298 содержат в `<style>` однодвоеточный псевдоэлемент. A/B одним бинарём: копии пар «тест + эталон» с `(?<!:):(before|after|first-line|first-letter)` → `::…` (не закоммичены, `.tmp/ab14.py`) — **208 из 298 → `identical`** при 800×600, 90 остаются `thick` (другие причины, см. BUG-1367, BUG-1369, `docs/wpt-vendor-notes/css.md` §срез 14). Основная масса — `generated-content/*` (`content-*`, `before-content-display-*`, `quotes-applies-to-*`) и `lists/counter-*`.

## Что делать

В `parse_pseudo`: если после одного `:` идёт `before` / `after` / `first-line` / `first-letter` (без `(`) — вернуть `SimpleSelector::PseudoElement(...)` так же, как для `::`. Для `is_valid_selector_list` (`selectors.rs:909`) однодвоеточная форма допустима там же, где двуколонная; `selector_query.rs` / `serialize` пишет её как `::before` (CSSOM). Остальные псевдоэлементы с одним двоеточием остаются невалидными.

## Как проверить

`css/CSS2/generated-content/content-003.xht`, `content-005.xht`, `lists/counter-increment-005.xht` — `reftest_pixdiff.py --viewport 800x600 --ahem` даёт `identical`; затем `run_corpus.py --prefixes css/CSS2/generated-content,css/CSS2/lists` — зелёных должно стать ≥ 200 больше.

## Срез 15 (2026-10-07, P2, WPT-RUN-14 `css/CSS2` selectors/css1/syntax/…)

Тот же дефект в остальных каталогах `css/CSS2`: 412 из 849 упавших reftest содержат однодвоеточный псевдоэлемент в `<style>`; **339 из них — `selectors/first-letter-punctuation-*`** (`div:first-letter`). A/B (`.tmp/ab15.py`, копии с `::`): **30 → `identical`** вне `first-letter-punctuation` (22 `selectors/first-letter-*`/`first-line-*`, 5 `cascade/*`, 3 `syntax/*`); у `first-letter-punctuation-*` ни один не стал `identical` — им мешают ещё [BUG-1373](BUG-1373-OPEN.md) (пунктуация после буквы) и [BUG-1374](BUG-1374-OPEN.md) (не-ASCII пунктуация). Закрытие — 30 id сразу и до 339 вместе с этими двумя.
