# BUG-1400 — `content-visibility: hidden`: геометрия потомков (`getBoundingClientRect`, `offset*`, `getClientRects`) — нули, принудительного layout нет

**Статус:** OPEN
**Заведён:** 2026-10-07 (P2, WPT-RUN-14 срез 17, `css/css-multicol` + `css/css-contain`)
**Область:** layout/js (`crates/engine/layout/src/content_visibility.rs`, `selector_query.rs` — геометрия скрытого поддерева; шим `getBoundingClientRect`)

## Симптом

`--dump-layout`, скрипт; `cv = div{content-visibility:hidden;width:100px;margin-left:20px}`, внутри `<div id=zz style="height:40px;width:30px;margin-left:5px">t</div><span id=sp>text here</span>`:

| запрос | получено | ожидается |
|---|---|---|
| `zz.getBoundingClientRect()` | `0,0,0×0` | `25,…,30×40` (поддерево раскладывается по запросу) |
| `zz.offsetWidth × offsetHeight`, `offsetLeft/Top` | `0×0`, `(0,0)` | `30×40`, `(5,…)` |
| `sp.getClientRects().length` | `0` | `1` |
| `zz.style.width='60px'; zz.getBoundingClientRect().width` | `0` | `60` |
| `getComputedStyle(zz).width` | `30px` | `30px` (верно) |
| `cv.getBoundingClientRect()` | `x=20, width=100, height=0` | (верно: сам элемент виден, содержимое не вносит высоту) |

## Как найдено

WPT-RUN-14 срез 17: 18 testharness-id в `css-contain/content-visibility/` (30 из 30 сабтестов — `assert_equals … expected 20 but got 0`, `Cannot read properties of undefined (reading 'width')`).

## Что делать

Запрос геометрии узла внутри скипнутого поддерева (`cv_is_skipped`) раскладывает это поддерево целиком, как `contentvisibilityautostatechange`-путь для `auto` уже делает для релевантного. Без записи в дисплей-лист: скипнутое остаётся нерисуемым.

## Как проверить

`css/css-contain/content-visibility/content-visibility-hidden-offsetTop-left-width-height.html`, `content-visibility-036.html`.
