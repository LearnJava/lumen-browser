# BUG-460 — column-direction flex: `align-items` other than `stretch` doesn't shrink-to-fit the cross size (width)

**Статус:** OPEN
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs::lay_out_flex`)
**Найден:** P1, 2026-07-31, при фиксе [BUG-425](BUG-425-OPEN.md)

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

This became directly visible only after [BUG-425](BUG-425-OPEN.md) fixed a
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
