# BUG-1483 — Поиск якоря по `anchor-name` игнорирует порядок в дереве: цель видит якорь, который стоит после неё (CSS Anchor Positioning 1 §2.2)

**Статус:** OPEN
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 21, `css/css-anchor-position` + `css/css-position` + `css/css-display`)
**Область:** layout (`crates/engine/layout/src/anchor.rs` — `collect_anchors`/поиск якоря по имени: порядок в дереве и область видимости имени)

## Симптом

Цель может ссылаться только на якорь, который в порядке дерева стоит раньше неё (и входит в допустимую область — содержащий блок цели и его предки с учётом `anchor-scope`). У нас якорь находится по имени независимо от положения: `<div style="position:relative"><div id=t class=target></div><div class=a style="width:20px"></div></div>` с `.target{position:absolute;width:anchor-size(--a width)}` даёт ширину 20, ожидается 0 (якорь после цели); то же для якоря во вложенном `position:relative` после цели и для цели во внутреннем `relative` с якорем во внешнем после неё. Когда якорь предшествует цели, ширина верна (20), а якорь внутри абсолютно позиционированного предка цели тоже находится верно (99). 8 id среза: `anchor-name-002.html` (2 из 6 сабтестов), `anchor-name-003.html` (29 из 39), `-004.html` (2 из 3), `anchor-name-mutation.html`, `anchor-name-005.html`, `anchor-size-001.html`, `anchor-query-fallback.html`, `mixed-dependency-chain.html` — по сообщению сабтеста («width expected 0 but got N», `data-expected-width=0`) и по пробе; полный состав не проверялся.

## Проба

Проба (`--mcp`, `.a{anchor-name:--a;height:10px}`, `.t{position:absolute;width:anchor-size(--a width);height:10px}`, якорь шириной 20 px):

| разметка | ширина цели у нас | ожидается |
|---|---|---|
| `relative` > `t`, затем `.a` | 20 | 0 |
| `relative` > (`t`), затем `relative` > `.a` | 20 | 0 |
| `relative` > `relative` > `t`, затем `.a` во внешнем | 20 | 0 |
| `relative` > `.a`, затем `t` | 20 | 20 |
| якорь внутри `absolute`-предка цели, перед ней | 99 | 99 |

## Как найдено

WPT-RUN-14 срез 21: `css-anchor-position/anchor-name-003.html` (29 из 39 сабтестов), `anchor-name-002.html`, `anchor-name-004.html`.

## Что делать

В `collect_anchors` при выборе якоря по имени отбрасывать кандидатов, которые в порядке дерева идут после цели, и проверять область имени (`anchor-scope`, содержащий блок).

## Как проверить

Таблица выше; `css/css-anchor-position/anchor-name-003.html`.
