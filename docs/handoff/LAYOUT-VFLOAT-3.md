# LAYOUT-VFLOAT-3 — handoff (ветка `p1-layout-vfloat-3`, слот `.claude/worktrees/p1-work`)

Задача: `ROADMAP.md:1039` (строка LAYOUT-VFLOAT-3). Файл удалить при закрытии задачи.

## Состояние

Кода ещё нет — сессия ушла на разведку. Ниже — результат разведки и принятый дизайн.
Бинарь базового состояния: `.tmp/lumen-base.exe` (в слоте; `.tmp/` не коммитится), сборка слота:
`cargo build --profile dev-release -p lumen-shell` (~4,5 мин холодная).

## Как проверять (A/B reftest)

`.tmp/ab.py` (в слоте, не в git; при потере — переписать по `tests/wpt/reftest_pixdiff.py`: `r.BIN`, `r.W,r.H=800,600`,
`r.ENV` = `LOCALAPPDATA=.tmp/fontsroot`, `LUMEN_CPU_SYSTEM_FONTS=1`, Ahem.ttf → `.tmp/fontsroot/Microsoft/Windows/Fonts/`,
`r.classify(path)` → `(path, 'identical'|'thick'|…, n)`). Запуск:
`python .tmp/ab.py '/css/css-shapes/shape-outside/shape-box/*border-radius-0[01]*.html' …`
Базовая картина (до правок): все 46 вертикальных тестов css-shapes `thick`; из 40 тестов `circle/` идентичны 3
(горизонтальные тоже падают: синтаксис `circle(50% at left 40px top 40px)`, `farthest-side`, shape-box не разбираются —
`register_shape_outside` понимает только `Npx`-формы). Список вертикальных тестов: `.tmp/vshapes.py`
(46 шт.: gradients 005–016, shape-box border-radius 005–012, circle 048–053, ellipse 046–051, inset 022–027,
polygon 020–025, shape-image-029 (url), shape-vertical-writing-mode-crash). Почти все — `display:inline-block`-ряды.

## Устройство (что выяснено)

- Контекст float-ов вертикального блока — `FloatContext` (`box_tree/shapes_floats.rs`) в логическом кадре:
  «y» = block-ось (физ. x; для vertical-rl от правого края content box), «x» = inline-ось (физ. y от верха).
  `float: left` = верх (`is_left` в ctx; у sideways-lr стороны меняются, `float_sides_swapped`).
- `vertical_float.rs::place_float` (вертикальный float): `fb` — block-позиция margin box, `i0` — inline-старт,
  `be`/`ie` — margin-box по block/inline; сейчас только `add_left/add_right`, контуры не регистрирует.
  Горизонтальный аналог — `block_flow_trampoline.rs::wire_shape_outside` → `register_shape_outside` (строка CSS).
- `rebased_block`/`translated`/`inheriting_vertical` (shapes_floats.rs ~585–620) сбрасывают контуры; `absorb_vertical`
  не переносит ни контуры, ни `shaped_left/right`.
- `vertical_trampoline.rs::step_child`: `run_floats` считается только для `InlineRun`
  (`vfloat::floats_for_run` → `rebased_block`); `InlineBlockRow` получает `None`.
- `vertical_row.rs::lay_out_vertical_inline_block_row`: колонки по inline-оси, перенос по `content_inline`, floats не знает.
- `vertical.rs::lay_out_vertical_inline_run`: колонка k = полоса `fc.line_band(k·lh, (k+1)·lh)`; для sideways-lr полосу
  надо зеркалить (`content_inline - r .. content_inline - l`) — пункт (3) задачи.
- Разбор basic-shape для clip-path (`style/parse/shape.rs`) ключевые слова позиции не понимает; для shape-outside
  годится только как образец. Градиенты: `crate::style::parse_background_gradient(s) -> ParsedGradient::Linear{angle_deg, corner,
  stops, ..}`, `GradientCorner::angle_deg(w,h)`, `GradientStop{color(.a: u8), position: Option<Length>}`.
- Радиусы: `ComputedStyle.border_{top_left,top_right,bottom_right,bottom_left}_radius` (+`_y`), `Length` (px/%).

## Принятый дизайн

1. **`box_tree/shape_resolve.rs` (новый)**: `Prim { Circle, Ellipse, Polygon(Vec<(f32,f32)>), RRect{x0,y0,x1,y1,radii:[(rx,ry);4]} }`
   с `extent_at(y) -> Option<(left,right)>`, `rotate(mw, rl)` (физ. (px,py) → логич. (py, rl ? mw-px : px); радиусы эллипсов/углов
   меняются осями), `translate`. `resolve_shape(sv, &FloatBoxes, shape_margin) -> Option<Vec<Prim>>`: разбор
   `<basic-shape> || <shape-box>` | `<shape-box>` (RRect с радиусами border-box/padding-box/content-box/margin-box) |
   `linear-gradient()` (полосы alpha>0 → полигоны: отсечение рамки двумя полуплоскостями перпендикулярно линии градиента);
   circle/ellipse: `at <position>` (1–4 токена, `left 40px top 40px`), `closest-side|farthest-side`, `%` от reference box
   (радиус круга `%` — sqrt((w²+h²)/2)); inset с `round a b c d / e f g h` (эллиптические радиусы, масштабирование при
   пересечении); polygon/path в координатах reference box. Координаты — от margin box (смещение reference box: border-box = (ml,mt) …).
2. **`FloatContext`**: новый вектор `shape_prims: Vec<GenShape{prim,is_left,top_y,bottom_y,bound_lo,bound_hi}>`; учесть в
   `left_edge_impl/right_edge_impl` (левый float: `extent.right.min(bound_hi)`, правый: `extent.left.max(bound_lo)`),
   `shapes_bottom`; `translated/rebased_block/absorb_vertical` переносят `shape_prims` и `shaped_*` (индексы + смещение).
3. **`vertical_float::place_float`**: собрать `FloatBoxes` из `child.rect` + margin/border/padding/radii, `resolve_shape`,
   `rotate`, `translate(i0, fb)`, окно `[fb, fb+be]`, `bound = [i0, i0+ie]`, `mark_last_shaped`.
   Горизонтальный путь не трогать: вызывать новый резолвер только если старый `register_shape_outside` вернул `false`
   (строго расширение, существующие случаи не меняются).
4. **`vertical_trampoline::step_child`**: `run_floats` и для `InlineBlockRow`; `layout_dispatch` передаёт `outer_floats` в
   `lay_out_vertical_inline_block_row`; в нём колонка k стартует на `taken`, полоса `fc.line_band(taken, taken+ширина колонки,
   0, content_inline)`, дети сдвигаются на `l`, первый элемент не влез → `taken = fc.next_float_bottom(taken)`;
   `InlineRun` в ряду — бюджет `r-l-cur.extent`. `rect.width` ряда = конец последней колонки.
5. Пункты (2) float посреди абзаца в горизонтальном потоке (брать по образцу `RunTail`), (3) зеркало полосы sideways-lr,
   (4) `contiguous-floated-table-*` — после основного.
6. Тесты юнит (`box_tree/tests/shapes_and_contain.rs`, новый `shape_resolve` тест-модуль) + `graphic_tests` если сдвинуты
   пиксели (`python graphic_tests/run.py --continue-on-fail`, голдены в том же коммите); документы: `subsystems/layout.md`
   (Done-запись), `CAPABILITIES.md`, ROADMAP-строка, BUG-1264 не трогать (он про flex).

## Ожидаемый ориентир (Done when)

circle-048, shape-box border-radius 005–012, gradients 005–016 — `identical` в `.tmp/ab.py`; остальные из 46 — по возможности.
Эталон circle-048: ряд колонок: длинный бокс на block 0 (контур там касательный), угловые боксы начинаются на inline 96/108/120.
