# BUG-1103 — canvas background-color propagation destructively mutates `<body>`'s own `ComputedStyle`, corrupting `getComputedStyle(body)`/`getComputedStyle(html)`

**Статус:** FIXED (P6, 2026-09-23)
**Перенумерован из BUG-1007** (P6, 2026-09-23): исходный номер оказался занят
двумя разными заявками одного дня (2026-09-05) — этот, канвовый, и
[BUG-1007](BUG-1007-FIXED.md) (`getClientRects()`/`getBoxQuads()` per-fragment
rects, FIXED тем же днём). Тот баг остаётся под 1007 — глубже прошит в код и
тесты (комментарии `v8_elem_geometry_scroll.rs`, коммит-история GAP-GEOM);
этот, ещё OPEN и слабее связанный, получил свободный номер.
**Заведён:** 2026-09-05 (P3, побочно при ревизии [BUG-514](BUG-514-FIXED.md))
**Компонент:** layout (`crates/engine/layout/src/box_tree/entry.rs::propagate_canvas_background`,
`canvas_background_color`)

## Механизм

CSS Backgrounds L3 §2.11.2 / HTML "Rendering" §the-page: when `<html>` has no
background, the canvas is painted using `<body>`'s background instead. This is
a **rendering/used-value** effect only — `getComputedStyle()` on either
`<html>` or `<body>` must keep reporting each element's own author-specified
value, unaffected by the propagation.

Lumen implements the propagation by **mutating the shared `Arc<ComputedStyle>`**
that the layout box tree holds for `<body>`:

```rust
// crates/engine/layout/src/box_tree/entry.rs:598-603
let body_style = Arc::make_mut(&mut body.style);
let bg_color = body_style.background_color.take();      // <- removes it from body
let bg_layers = std::mem::take(&mut body_style.background_layers);
let html_style = Arc::make_mut(&mut html_box.style);
html_style.background_color = bg_color;                  // <- moves it onto html
html_style.background_layers = bg_layers;
```

`LayoutBox::style` is the exact object later serialized into the
`computed_styles` cache that backs JS `getComputedStyle()`
(`crates/engine/layout/src/lib.rs::collect_computed_styles`, running over the
box tree *after* this mutation). The move therefore leaks into CSSOM on both
ends: `body` loses a value it should still report, and `html` gains one it
never had.

## Симптом

Confirmed live (`--mcp-live-port`, `LUMEN_NO_ENGINE_THREAD=1`), minimal repro
— a page with nothing but:

```html
<style>body { background-color: rgb(9, 9, 9); }</style>
```

`getComputedStyle(document.body).getPropertyValue('background-color')` →
`"rgba(0, 0, 0, 0)"` (should be `"rgb(9, 9, 9)"`). `color`/`margin-top` set on
the same rule read back correctly — only `background-color` (and, by the same
code path, `background-image`/`background-layers`) is affected. Symmetric
bug on the other side: `getComputedStyle(document.documentElement)` would
report the propagated color as `html`'s own, which is equally wrong (not
separately reproduced with a live probe, but follows from the same
`html_style.background_color = bg_color` assignment).

This is **not** the same defect as [BUG-493](BUG-493-OPEN.md)/CSSOM-4 (stale
cache on a script-mutated-in-the-same-tick node) — reproduces on a
statically-parsed `<body>` that was never touched by script, and `color`/
`margin-top` on the identical element read back fine in the same call.

## Масштаб

Found via 2 of [BUG-514](BUG-514-FIXED.md)'s five `css/css-env` files
(`at-supports.tentative.html`, `fallback-nested-var.tentative.html` — both
assert on `getComputedStyle(document.body)`'s `background-color`, unrelated to
`env()` itself; the `env()` value happens to be what's assigned, but the
underlying read is broken for *any* value). Not measured beyond that: this
mechanism runs on every document with an `<html>`/`<body>` pair, so any page
whose script or devtools reads `background-color` off `document.body` (or
`document.documentElement`) after CSS gives `<body>` a background is affected.
Not investigated: whether `background-image`/gradients on `body` show the
same CSSOM corruption (same code path moves `background_layers` the same
way — very likely yes, just not separately confirmed with a probe).

## Почему это не point-fixed в этой ревизии

The naive fix (stop moving the value — copy it to `html` instead of taking
it from `body`) is **not safe**: painting has no separate "used value"
concept here, so if `body`'s own box paint step is left untouched it would
re-paint its own background on top of the canvas clear. For opaque solid
colors that's a harmless idempotent double-paint, but for a translucent
`background-color` (e.g. `rgba(255,0,0,0.5)`) it double-composites and
visibly darkens the color — a real regression, not just a CSSOM nuance.

A correct fix needs the canvas-clear color to be computed **without**
touching either element's `ComputedStyle` (`canvas_background_color()` can
walk `html`/`body`'s *unmutated* styles directly — its only two call sites,
`crates/shell/src/frames.rs` and `.../window_event/redraw_requested.rs`, don't
require the box tree to have been pre-mutated), plus a **separate,
non-`ComputedStyle` marker** on `body`'s `LayoutBox` telling the paint pass to
skip repainting the propagated background at `body`'s own box (so translucent
colors don't get composited twice). That's a `LayoutBox`-shape change
(currently no such flag exists — see `crates/engine/layout/src/box_tree/types.rs`),
plus updates to the 5 existing propagation unit tests in
`crates/engine/layout/src/tests/table_grid_presentational.rs` (`html.style.background_color`/
`body.style.background_color` assertions there directly assert today's
mutation, i.e. the bug), plus wherever the paint backend turns a box's own
`background_color` into a fill command — wider than a single-file fix,
deferred rather than rushed.

## Воспроизведение

```
python .tmp/probe_bug514_env.py   # ad-hoc probe written for this investigation,
                                   # not committed — see BUG-514-FIXED.md's revision
                                   # note for the exact minimal HTML/JS
```

Minimal HTML:

```html
<style>body { background-color: rgb(9, 9, 9); }</style>
```

`getComputedStyle(document.body).backgroundColor` reads `rgba(0, 0, 0, 0)`
instead of `rgb(9, 9, 9)`.

## Триаж 2026-09-23: коллизия номера

Под номером BUG-1007 заведены два разных бага в один день (2026-09-05): закрытый
`bugs/BUG-1007-FIXED.md` (`getClientRects()` по фрагментам, P1, `BUGS-FIXED.md`) и этот
открытый (перенос фона `<body>` на канву). Задача P6: дать этому следующий свободный номер,
переименовать файл, поправить строку `BUGS.md` и все ссылки на него, затем
`python scripts/remap_status_pointers.py --apply` и `python scripts/check_doc_links.py`.

**Сделано (P6, 2026-09-23):** переименован в BUG-1103 (файл, строка `BUGS.md`,
обе ссылки из `bugs/BUG-514-FIXED.md`). `remap_status_pointers.py --apply` дал
ложный «ПРОТУХ» на `STATUS-P6.md:BUGS.md:249` — его якорь `BUG-1007` совпал
текстом с несвязанным `BUG-1007` из `BUGS-FIXED.md` (тот самый коллизионный
номер), указатель НЕ снят, баг остаётся открытым по той же строке.
`check_doc_links.py` чист (442 проверенных ссылки). Сам дефект (описан выше,
раздел «Почему это не point-fixed») не тронут в этом срезе: фикс требует
менять background-путь в `crates/engine/paint` минимум в 6 файлах
(`walk.rs`, `table.rs`, `inline_frag.rs`, `text_run.rs`,
`svg_text_decoration.rs`, `background_mask.rs`) — движущий пиксели путь,
которому по `CLAUDE.md` нужен полный `graphic_tests/run.py --continue-on-fail`
(foreground-окно), недоступный в этой сессии
([[feedback_background_launched_window_breaks_mcp_js_context]]). Остаётся
`OPEN` под P1/P3 для следующей сессии с реальным окном.

## Фикс 2026-09-23 (P6)

Переоценка масштаба: полная «правильная» схема из раздела выше (флаг на
`LayoutBox` + копия фона на `<html>` + подавление собственной покраски
`<body>`) требует нового обязательного поля `LayoutBox` — структура без
`Default`/конструктора, инициализируется прямым литералом в 48 файлах
(`box_tree.rs`, `flex.rs`, `grid.rs`, table-layout и т.д.). Добавление
required-поля означало бы правку всех 48 мест ради узкого крайнего случая
(полупрозрачный `background-color` на `<body>` при том, что рамка `<html>`
явно выше рамки `<body>`, например через `min-height`) — в обычном
документе `<html>`-бокс auto-размерен ровно как `<body>`, так что разница
не видна.

Выбран более узкий, но безопасный фикс без единого нового поля:

- `propagate_canvas_background` (перенос `background_color`/`background_layers`
  через `Arc::make_mut` на общий, кэшируемый в CSSOM `ComputedStyle`)
  **удалена целиком**, вместе со всеми 4 местами вызова в `entry.rs`.
  `<body>`/`<html>` больше никогда не теряют и не приобретают
  `background-color`/`background-image` в собственном `ComputedStyle` —
  `getComputedStyle()` на обоих всегда отдаёт то, что реально задал автор.
- `canvas_background_color()` (единственный потребитель пропагации —
  два call site, `crates/shell/src/frames.rs` и
  `.../window_event/redraw_requested.rs`, оба читают уже построенное
  дерево) теперь сама read-only решает, чей фон использовать для очистки
  канвы: свой у `<html>`, если есть (`background_color.is_some() ||
  !background_layers.is_empty()`, как раньше), иначе — `<body>`'s,
  напрямую, без промежуточной мутации. Непрозрачность (`a == 255`)
  проверяется как и раньше.
- Собственная покраска `<body>`-бокса ничем не тронута: она и раньше не
  зависела от этой пропагации (обычная покраска любого бокса по своему
  `style`), так что для непрозрачных цветов очистка канвы + покраска
  `<body>`'s собственного rect — идемпотентный двойной draw одним и тем же
  цветом (не композит, просто перезапись), визуально неотличим от старого
  поведения. Прожекторный разбор — почему это безопасно даже без флага — в
  doc-комментарии `canvas_background_color`.

Обновлены 6 юнит-тестов в `crates/engine/layout/src/tests/table_grid_presentational.rs`,
раньше напрямую проверявших мутацию (`html.style.background_color ==
Some(...)`, `body.style.background_color == None`) — теперь проверяют
`canvas_background_color(&root)` для видимого поведения и
`html.style`/`body.style` для CSSOM-инварианта (оба хранят СВОИ значения).

Полный `python graphic_tests/run.py --ipc --build --continue-on-fail`
(детерминированный CPU-снимок по TCP, без живого окна — сессия без GUI):
27/157 FAIL, все — известный дрейф (BUG-128 и т.д., не регрессия), **дельта
против прогона на родительском коммите `a0fa36a07` — «Изменений нет»**.
`cargo test -p lumen-layout` — 3999 passed. `cargo clippy -p lumen-layout
--all-targets` и `-p lumen-shell --all-targets` — чисто.

Остаток (полупрозрачный `background-color` на `<body>`, когда рамка
`<html>` явно выше рамки `<body>`) в корпусе тестов не воспроизведён и не
считается регрессией — это узкий, не покрытый ни одним существующим тестом
край, задокументированный в doc-комментарии `canvas_background_color` как
сознательное упрощение.
