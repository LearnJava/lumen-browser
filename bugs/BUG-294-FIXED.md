# BUG-294 — flex row items with `margin-left`/column items with `margin-top` are positioned with the margin applied twice

**Статус:** FIXED 2026-07-19
**Компонент:** layout (`crates/engine/layout/src/box_tree.rs`, `lay_out_flex`)
**Найден:** P2-DEVX-2, писал non-pixel golden-regression тест на `.item-b { margin-left: 10px }` внутри `display:flex` row-контейнера

## Фикс (P3, 2026-07-19)

Both the row and the column arm of `lay_out_flex`'s final-positioning loop now pass the
item's **bare margin-box origin** to `lay_out` (`content_x` / `content_y + main_cursor` in
the column arm; `content_x + main_cursor` / `content_y + cross_cursor` in the row arm),
matching every other `lay_out` call site in the file. `lay_out_inner` adds the box's own
`margin_left`/`margin_top` exactly once. The main-axis accumulator was already correct and
is untouched.

Observability of the double-add differed per axis, which is why the regression tests target
specific axes:
- **Row `margin-left`** (main axis) — never rewritten after layout → double-add surfaced
  directly in `rect.x` (`flex_row_item_margin_left_applied_once`).
- **Column `margin-top`** (main axis) — column containers skip the cross-alignment pass →
  `rect.y` double-add observable (`flex_column_item_margin_top_applied_once`).
- **Column `margin-left`** (cross axis, no cross-alignment pass for column) → `rect.x`
  double-add observable (`flex_column_item_margin_left_applied_once`).
- **Row `margin-top`** was *not* observable in the item's own `rect.y` (the cross-alignment
  pass overwrites it to `content_y + cross_cursor + m_t`), but the extra offset leaked into
  the item's already-positioned descendants — fixed by the same change.

## Симптом

A flex item in a row container with a non-zero `margin-left` ends up positioned 2×`margin-left` to
the right of the preceding item's border-box edge, instead of 1×.

Repro fixture (`crates/driver/tests/fixtures/golden-containers.html` in this branch, before it was
adjusted to route around the bug):

```html
<div class="flex-row" style="display:flex; width:300px">
  <div class="flex-item item-a" style="width:60px; height:40px"></div>
  <div class="flex-item item-b" style="width:60px; height:40px; margin-left:10px"></div>
</div>
```

Observed via `InProcessSession::layout_box_by_selector`: `item-a.border_box.x = 12`,
`item-a.border_box.width = 60`, `item-b.border_box.x = 92`. Expected `item-b.x = 82`
(`12 + 60 + 10`). Gap between `item-a`'s right edge and `item-b`'s left edge is `20px`, not the
authored `10px`.

## Причина (confirmed by code reading)

In `lay_out_flex`'s row branch (`box_tree.rs`, main-axis positioning loop, non-`is_column` arm), the
child is laid out with:

```rust
let m_l = item_s.margin_left.resolve_or_zero(iem, cb, viewport);
...
lay_out(
    &mut children[i],
    content_x + main_cursor + m_l,   // <- margin already added here
    content_y + cross_cursor + m_t,  // <- margin_top already added here too
    inner_main,
    ...
);
```

But `lay_out`/`lay_out_inner` (the generic recursive box-layout entry point used by every call site
in the file, e.g. the plain block-flow child loop around `box_tree.rs:6742` which passes bare
`content_x` with no margin pre-added) **always** resolves and applies the box's own margin against
the `start_x`/`start_y` it receives:

```rust
let margin_left = s.margin_left.resolve_or_zero(em, cb, viewport);
...
b.rect.x = start_x + margin_left;
b.rect.y = start_y + margin_top;
```

So the flex row/column branches' explicit `+ m_l` / `+ m_t` double-counts the margin that
`lay_out_inner` is about to add again. Every other call site in the file (block flow, table cells,
grid, `lay_out_flex`'s own preliminary pass at `box_tree.rs:8163`) passes the *margin-box* start
(no margin pre-added) and lets `lay_out_inner` add it — the flex final-positioning loop is the
outlier.

The main-axis accumulator (`main_cursor += outer_main + item_gap + jc_gap`, where `outer_main`
already includes the item's own margins) is itself correct — only the position handed to `lay_out`
for that item is wrong. `margin-right`/`margin-bottom` are not doubled (they only ever enter via
`outer_main`, never re-added to a start coordinate).

Likely also affects:
- `margin-top` on row-direction flex items (`content_y + cross_cursor + m_t` — same pattern, just
  untested so far since no existing flex test in `box_tree.rs` sets item margin, they all use `gap`).
- `margin-left` on column-direction flex items (`content_x + m_l` in the `is_column` arm — same
  double-add against `lay_out_inner`'s own `margin_left`).

No existing test in `crates/engine/layout/src/box_tree.rs`'s flex test suite (`flex_align_content_*`,
`flex_nowrap_*`, etc.) sets margin on a flex item — they all rely on `gap`/`justify-content` for
spacing, which is why this went uncaught until now.

## Обход в DEVX-2 golden-тесте

`crates/driver/tests/cases/test_devx2_golden.rs` / `crates/driver/tests/fixtures/golden-containers.html`
use `gap` instead of `margin-left` for the flex-row item-spacing assertion, to avoid baking this bug
into a "golden" regression baseline. Revisit once fixed — `gap` and per-item `margin` should both be
covered by that test file.
