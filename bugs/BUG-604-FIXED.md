# BUG-604: no UA (user-agent) shadow tree for `<video>`/`<audio>`/`<select>`/`<details>` — light-DOM children render directly instead of being hidden/slotted per spec

**Статус:** FIXED 2026-09-26 (P1, GAP-UASHADOWSLOT)
**Компонент:** dom (`crates/engine/dom/src/lib.rs` — `Document::create_element`/`try_create_element`, `ua_shadow_kind`/`attach_ua_shadow_root`)
**Найден:** P2, WPT-VENDOR-html-rendering, 2026-08-04

## Симптом

```
FAIL <video></video> has a shadow tree with no slots - child.getClientRects is not a function
FAIL <select></select> has a shadow tree with slot - assert_not_equals: child should be in the flat tree got disallowed value 0
```
(`widgets/shadow-dom.html` — the `getClientRects` `TypeError`s are
[BUG-478](BUG-478-FIXED.md)/[BUG-522](BUG-522-FIXED.md)/[BUG-551](BUG-551-DUPLICATE.md)/[BUG-580](BUG-580-DUPLICATE.md)
territory and the `outerHTML`-in-test-name-collision harness `ERROR` is
[BUG-351](BUG-351-OPEN.md); this bug is the *assertion content* underneath
both, once those are stripped out)

## Причина

HTML LS §4.8.11(video/audio)/§4.10.11(select)/§4.11.1(details) specify each
of these elements ships with an internal ("UA") shadow tree: `<video>`/
`<audio>` render their built-in controls through one with **no slot at
all**, so any light-DOM child a page appends is never part of the flat tree
— `getClientRects().length` must be `0` and `getComputedStyle(child).length`
must be `0` (element outside the rendered tree). `<select>`/`<details>`
instead have a shadow tree **with** a slot, so an appended child *is*
rendered but inherits from the slot per the UA stylesheet
(`display: contents` etc.), not from ordinary cascade rules.

Lumen has no internal shadow-root construction for any of these four
interfaces — a `<span>` appended to a live `<video>` becomes an ordinary
rendered light-DOM child (violates the "no slot" contract), and one
appended to `<select>`/`<details>` does not go through the expected
UA-stylesheet-driven slot inheritance path either. This is a gap in the
element implementations themselves, not in the generic Shadow DOM machinery
(author-created `attachShadow` shadow roots work correctly elsewhere in the
corpus).

## Масштаб

Architectural — needs an actual internal shadow root per interface, wired
into each element's construction, not a display-property tweak. Confirmed
narrowly (4 elements, 1 file, 9 subtests) in this slice; likely affects any
other WPT test that assumes UA shadow tree encapsulation for these same
four elements elsewhere in the vendored corpus (not swept beyond this file).

## Срез P3 2026-09-13

Independently re-verified before touching anything: the general Shadow DOM
machinery (`Document::attach_shadow`/`build_flat_tree`/
`compute_slot_assignments` in `crates/engine/dom/src/lib.rs`) is fully
implemented and tested — the "architectural gap" framing above overstated
the remaining work. `compute_slot_assignments`'s existing rule ("children
with no matching slot are not rendered in the flat tree") already gives
exactly the `<video>`/`<audio>` "no slot" semantics for free once a shadow
root is attached with no `<slot>` child inside it — no new mechanism needed.

**Landed (`<video>`/`<audio>`):** `Document::create_element`/
`try_create_element` now call `attach_ua_shadow_root` for these two tags
(`ua_shadow_kind`), attaching an empty `Closed` shadow root at construction
time regardless of creation path (HTML parser or `createElement`). Since
`<video>`/`<audio>` are opaque replaced boxes in layout (`BoxKind::Video`/
`BoxKind::Audio` never walk DOM/flat-tree children for their own geometry —
`crates/engine/layout/src/box_tree/build.rs`), this has zero effect on
existing page layout; it only changes what `build_flat_tree` reports for a
light-tree child appended to one of these elements, which is exactly the
spec-required "never part of the flat tree" behavior the WPT assertion
checks. 5 new unit tests in `crates/engine/dom/src/lib.rs` (shadow-host
attachment for both tags via both creation paths, ordinary elements
unaffected, a `<span>` child of `<video>` excluded from
`build_flat_tree`'s output). Full `cargo test -p lumen-dom`/
`-p lumen-layout`/`-p lumen-js --features v8-backend`, `cargo clippy -p
lumen-dom --all-targets -- -D warnings`: all green, no regressions.

**Not landed (`<select>`/`<details>`):** giving these two a real UA shadow
tree needs the slot's *content* to render without the `<slot>` element
itself generating a box (HTML/CSS: `<slot>`'s default `display` is
`contents`). This engine's `Display::Contents` is parsed/stored but laid
out as plain `Block` (deferred — see `enum Display` doc comment,
`crates/engine/layout/src/style/values/typography.rs`), so a real `<slot>`
would insert a spurious visible wrapper box into *every* `<select>`/
`<details>` on *every* page — `<details>` is not opaque like `<video>`
(its content really does flow through block-flow layout,
`is_details_element` branch, `box_tree/build.rs:645`), so this is a genuine
pixel-moving change far outside this bug's original 9-subtest blast
radius, gated by a separate, real `display: contents` implementation.
Reclassified into [GAP-UASHADOWSLOT](../ROADMAP.md) per the same rule as
BUG-534/553/562/583 (absent primitive + family-sized, not a point fix).

## Закрытие 2026-09-26 (P1, GAP-UASHADOWSLOT)

`ua_shadow_kind` отдаёт вариант со слотом для `<select>` (`UaShadowKind::Contents`, один
слот) и `<details>` (`UaShadowKind::Details`, слот summary + content-слот). Слоты заполняются
по позиции (`ua_slot_assignments`: первый дочерний `<summary>` — в первый слот, остальное — во
второй), атрибут `slot` на детях игнорируется. Корень закрытый, `cloneNode` даёт клону свой.
UA-стили: `slot { display: contents }` для любого слота (`default_display`), content-слот
`<details>` — `display: block` + `content-visibility: hidden` без `open` (`apply_ua_slot`,
ключ — `Document::ua_slot_role`, т.к. ни один селектор узел закрытого UA-дерева не назовёт).
Правила документа до UA-слота не доходят (CSS Scoping L1 §3.1 — в `compute_style` сопоставленные
декларации для UA-слота сбрасываются; авторские shadow-деревья правила документа по-прежнему
видят — это более широкий старый пробел). Сужение рестайла BUG-341 (`restyle.rs`
`document_has_shadow_roots`) считает только авторские корни (`Document::has_author_shadow_roots`):
у UA-деревьев нет листа, а иначе сужение выключилось бы на любой странице с `<select>`, включая
chrome Lumen. Старый фильтр «закрытый `<details>` строит только `<summary>`» в `box_tree/build.rs`
остался для `<details>` без UA-корня. A11y-дерево разворачивает `<slot>` в его содержимое. S27-хребет
инкрементального рестайла идёт по `FlatTree::parent_of`, а не по DOM-родителю — иначе он
отключался бы на любой странице с `<select>`.

A/B `--dump-display-list` по 180 страницам `graphic_tests/` + `samples/`: 0 различий
(пиксельный прогон недоступен — TEST-00 не находит маркер, захват экрана сломан в этой
сессии). Ручная страница с `<details>`: изменился только `details { display: flex }` —
содержимое теперь один flex-item (content-слот), как у Chrome.

Оставшиеся 5 подтестов `widgets/shadow-dom.html` упираются в другое: пустой `<span>` не
публикует computed style ([BUG-1191](BUG-1191-OPEN.md)), а `all: inherit` каскад не
применяет (GAP-CSSALL). Проверено юнит-тестами с непустым `<span>` и поштучным `inherit`
(`crates/js/src/dom/tests/v8_gap_uashadowslot.rs`).
