# BUG-460 — column-direction flex: `align-items` other than `stretch` doesn't shrink-to-fit the cross size (width)

**Статус:** FIXED (обнаружено ревизией) 2026-08-31
**Компонент:** layout (`crates/engine/layout/src/box_tree/flex.rs::lay_out_flex`)
**Найден:** P1, 2026-07-31, при фиксе [BUG-425](BUG-425-FIXED.md)

## Симптом

For a `flex-direction: column` container, the cross axis is width. When
`align-items` (or `align-self`) is `center` / `flex-start` / `flex-end` (i.e.
anything other than `stretch`), a real browser sizes each item to its own
content (shrink-to-fit) and then positions it along the cross axis per the
alignment keyword. Lumen currently has no such handling: the code path that
resolves cross-axis alignment for a flex line is gated on `!is_column` —
`lay_out_flex`'s comment says outright *"column cross axis (width) not
handled in wrap Phase 0"* (`box_tree.rs`, the `line_cross` computation right
above the `if !is_column { … AlignValue::Stretch … }` block).

In practice every non-replaced block-level item in a column flex container
ends up filling the full container width regardless of `align-items`,
because ordinary block auto-width (`available_width - margins`, applied
whenever the item has no explicit CSS `width`) already fills the container —
there is no code that would instead give it a shrink-to-fit width and center/
offset it.

## Repro

`assets/chrome/chrome.html`: `.newtab{ display:flex; flex-direction:column;
align-items:center; }` contains `<button class="nt-restore">…Восстановить
закрытые</button>` with no explicit `width`. Expected (matches
`docs/design/lumen-v3_3.html` opened in a real browser): a content-sized,
horizontally-centered pill button. Actual: the button stretches to the full
width of `.newtab`'s content box.

```bash
cargo build -p lumen-shell --profile dev-release
./target/dev-release/lumen.exe --dump-layout assets/chrome/chrome.html | grep -A2 'Восстановить закрытые'
```

## Note

This became directly visible only after [BUG-425](BUG-425-FIXED.md) fixed a
related-but-distinct defect: `BoxKind::FormControl` (`<button>`, `<select>`)
was unconditionally treated as a CSS "replaced element" for auto-width
purposes, so it got `width: 0` instead of filling available space — for
`.nt-restore` that accidentally produced a *narrower* box than this bug
would, masking it. Now that FormControl auto-width follows normal block
rules when its `display` is `flex`/`grid`, `.nt-restore` exposes the full
column cross-axis alignment gap described here. `.ws-add` (the other label
BUG-425 flagged) is unaffected because its container, `.sb-workspaces`, uses
the default `align-items: stretch`, which *is* implemented — just not for
`is_column` (that gap is also part of this bug: worth checking whether the
`AlignValue::Stretch` cross-size logic mirrored from the row-direction branch
should share code, once written).

## Направление разбора

Mirror the existing row-direction cross-axis-alignment block (`if !is_column
{ … }`, handles `AlignValue::End/Center/Stretch` by moving `item.rect.y` and
optionally growing `item.rect.height`) with a column-direction counterpart
that operates on `item.rect.width`/`item.rect.x`, using `content_width` as
the (always-definite, unlike row's `explicit_cross`) cross size. `Stretch`
grows the item to `content_width` (already effectively true for non-replaced
blocks today — the new code must not double-grow them); `Center`/`End`
require first computing the item's own shrink-to-fit width (today items are
laid out at content-width-minus-margins unconditionally in the column final
pass, `box_tree.rs` around `lay_out(&mut children[i], content_x, content_y +
main_cursor, content_width - m_l - m_r, …)`) — that call would need to pass a
narrower/shrink-to-fit width for non-stretch alignments instead.

## Резолюция (P3-ревизия, 2026-08-31)

Ревизия строки STATUS-P3.md обнаружила, что описанный дефект уже устранён —
не под этим номером бага. Коммит `e9566ca38` (2026-08-18, «Флекс: auto-поля,
потолок роста и поперечная ось колоночного контейнера», разбирал независимый
живой случай `tbank.ru/login/`) добавил в `lay_out_flex` именно ту поперечную
ось колоночного контейнера, которую этот баг описывал: `cross_align`
(`align_self` поверх `align_items`), `aligned_cross` (Start/End/Center) даёт
`used_cross` через `max_content_outer_width`/`min_content_outer_width`
(shrink-to-fit) вместо безусловного `avail_cross`, плюс позиционирование
`cross_shift` по тому же правилу, что и строчный случай. Задача не
упоминала BUG-460 явно — найдена по совпадению формулировки «поперечная ось
колоночного контейнера» с текстом этого бага, не по номеру.

Проверено репро из этого файла (`--dump-layout assets/chrome/chrome.html`):
`.nt-restore` теперь `FormControl rect=(538.44, 315.08, 187.12, 21.00)`
внутри родителя `.newtab` `Block rect=(240.00, 36.00, 784.00, 320.08)`
(content box `x ∈ [260, 1004]`, padding 20px) — центр кнопки
538.44 + 187.12/2 = 632.0 точно совпадает с центром контейнера
240 + 784/2 = 632.0, при ширине 187.12px вместо растяжения на все 744px
content-ширины. Соответствует ожиданию из `## Repro`.

Тесты, добавленные 2026-08-18, покрывали только ветку auto-margin
(`margin: auto` на поперечной оси); голый `align-items`/`align-self` без
auto-полей и без явного `width` — точный случай этого бага — не имел
регресс-теста. Добавлены три в
`crates/engine/layout/src/box_tree/tests/flex_align_content.rs`:
`flex_column_align_items_center_shrinks_to_fit_and_centers`,
`flex_column_align_items_flex_end_shrinks_to_fit_and_pushes_to_end`,
`flex_column_align_self_center_overrides_container_align_items`.

`.ws-add`/`align-items: stretch` (упомянутый в `## Note`) не проверялся
отдельно этой ревизией — вне репро этого файла.
