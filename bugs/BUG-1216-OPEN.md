# BUG-1216 — `@scope` без явного root резолвит имплицитный корень как документ, а не как родителя листа

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/style/env.rs:275-276` — `node_in_scope`,
`crates/engine/layout/src/style/cascade.rs:654-660` — вызов из применения `scope_rules`)
**Найден:** P6, 2026-09-29, побочно при верификации фикса BUG-967

## Механизм

`@scope { … }` без явного `(<root>)` — CSS Cascade-6 §3 определяет имплицитный
корень как **родителя владеющего узла листа стилей** (`the parent of the style
sheet's owner node`), а не как корень документа. `node_in_scope`
(`env.rs:275-276`) считает иначе:

```rust
// No explicit root: implicit document-root scope (only limits cut off).
root_empty
```

— при пустом `root_sel_str` функция возвращает `true` для ЛЮБОГО узла, если он
не отрезан лимитом. Функция вообще не получает владеющий узел листа (ни как
параметр, ни через `Stylesheet`) — `node_in_scope(doc, node, &scope_rule.root,
scope_rule.limit.as_deref())` в `cascade.rs:660` не имеет доступа к тому,
откуда взялся `sheet`/`scope_rule`, так что даже плюмбинг для правильного
резолва сейчас негде закрепить без изменения сигнатуры.

## Симптом

Живой пробой (`tests/wpt/verify_slice54_gaps.py`, `css/css-cascade/
scope-implicit-external.html`, subtest «@scope with external stylesheet through
link element»): лист `resources/scope.css` содержит

```css
@scope { :scope { z-index:1 } .a { z-index:2 } }
```

без `(<root>)`, подключён через `<link>` внутрь `<div id=root>` с одним `.a`
потомком. Ожидание (Chrome, спека): `z-index:2` только у потомка внутри
`#root`; два `.outside` `<div class="a outside">` снаружи `#root` — `auto`.
Фактически (Lumen, `harness-complete` подтест `:1` = FAIL):
`getComputedStyle(div).zIndex` для обоих `.outside` тоже читает `"2"` —
имплицитный корень трактуется как документ целиком, `.a`-правило утекает мимо
границы, которую `<link>`'s владеющий узел (внутри `#root`) должен был
поставить.

Тот же лист через `@import` внутри `<style>` (subtest «… through @import»,
BUG-967) даёт идентичный FAIL после фикса BUG-967 — раньше маскировался
TIMEOUT'ом самого BUG-967, теперь оба subtest'а завершаются штатно
(`harness-complete status=0`) и оба FAIL на этой самой ассерции.

## Масштаб

Любой `@scope { … }` без явного `(<root>)` в подключённом (`<link>`/`@import`)
листе резолвится как «весь документ», а не «поддерево владеющего элемента» —
затрагивает произвольную разметку, использующую имплицитный `@scope` в
отдельном CSS-файле, а не только этот WPT-тест.

## Что нужно

Пробросить владеющий узел листа (элемент `<link>`/`<style>`, для inline —
сам стиль-блок) до `node_in_scope`, чтобы при пустом `root_sel_str` она
проверяла «`node` — inclusive descendant родителя владеющего узла» вместо
безусловного `true`. Требует сигнатурных изменений в `node_in_scope` и в
месте вызова из `cascade.rs`, плюс, вероятно, поле в `Stylesheet`/
`ScopeRule` с id владеющего узла — не тронуто в этой сессии (не в рамках
BUG-967, самостоятельный дефект, `docs/probe-method.md` §3: один id, ≥1
дефект).

## Классификация WPT

`css/css-cascade/scope-implicit-external.html`, оба subtest'а («through link
element», «through @import») FAIL на этой ассерции после фикса BUG-967;
раньше «through link element» уже был известен как FAIL (замечено в
[BUG-967](BUG-967-FIXED.md)), «through @import» был замаскирован TIMEOUT.
