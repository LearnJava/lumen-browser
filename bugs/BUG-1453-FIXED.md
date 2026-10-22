# BUG-1453 — В таблице стилей теневого дерева селектор `:host <потомок>` (`:host div.red`, `:host > div`, `:host .x`, `:host(.x) div`) не применяется; `:host {…}` применяется

**Статус:** FIXED 2026-10-22 (P3)
**Заведён:** 2026-10-08 (P2, WPT-RUN-14 срез 20, `css/selectors` + `css-pseudo` + `css-nesting` + `css-namespaces` + `css-cascade`)
**Область:** layout/css-parser (`crates/engine/layout/src/style/matching/` — `:host` в составных селекторах теневого дерева)

## Симптом

Для `<div id=h><template shadowrootmode=open><style>:host div.red{background-color:green} div.red{background-color:red}</style><div class=red id=r>` итоговый `background-color` — `rgb(255, 0, 0)` (правило `:host div.red` не сработало, хотя его специфичность выше); `:host{color:green}` работает. То же без `div.red`: `:host div`, `:host > div`, `:host .x`, `:host(.x) div` → фон прозрачный. `complex_has_host` (`matching.rs:163`) выбирает из листа только правила, чей ПОСЛЕДНИЙ компаунд содержит `:host`, — правила, где `:host` стоит слева от потомка (а субъект — потомок в тени), не сопоставляются. Страдают 6 reftest `featureless-001…005` и `scope-part` (все `thick`); по спецификации (Scoping 1 §3.2) `:host` в таких селекторах совпадает с хостом, а сам селектор — с потомками в тени.

## Проба

`getComputedStyle(shadowRoot.querySelector("#r")).backgroundColor`, ожидается `rgb(0, 128, 0)`:

| правило в `<style>` теневого дерева | у нас |
|---|---|
| `:host{display:block;color:green}` (`getComputedStyle(h).color`) | `rgb(0, 128, 0)` — верно |
| `:host div.red{background-color:green}` + `div.red{background-color:red}` | **`rgb(255, 0, 0)`** |
| `:host div{…}` | **`rgba(0, 0, 0, 0)`** |
| `:host > div{…}` | **`rgba(0, 0, 0, 0)`** |
| `:host .x{…}` (`<div class=x>`) | **`rgba(0, 0, 0, 0)`** |
| `:host(.x) div{…}` (`<div id=h class=x>`) | **`rgba(0, 0, 0, 0)`** |

## Как найдено

WPT-RUN-14 срез 20: `selectors/featureless-001.html` … `featureless-005.html`, `css-cascade/scope-part.html`.

## Что делать

При разборе листа теневого дерева различать правила «`:host` в субъекте» и «`:host` — предок» и для вторых сопоставлять цепочку с хостом как с корнем (`:host` ≡ хост, комбинатор идёт к потомку в тени).

## Как проверить

Таблица выше; `css/selectors/featureless-001.html`.

## Исправление

Причина: у `ShadowRoot` нет `parent`, поэтому обход предков в `matches_chain` (`style/matching.rs`) никогда не доходил до хоста, а `:host` ещё и включался только для подлежащего (`SHADOW_HOST_SCOPE` ставился лишь при каскаде самого хоста). Теперь `cascade.rs` на время матчинга листа внутреннего узла ставит `SHADOW_HOST_SCOPE` = охватывающий хост, а `Descendant`/`Child` при упоре в `ShadowRoot` переходят к хосту (`scoped_shadow_host`). Вне такого скоупа поведение прежнее (`:host` в документе — no-op; светлые дети хоста и документные правила не затронуты). Тесты: `style/tests/shadow_dom_selectors.rs` (`host_descendant_*`, `host_child_*`, `host_function_with_descendant_*`).
