# BUG-553: CSS Gap Decorations implemented under non-spec property names (`gap-rule*` instead of `column-rule*`/`row-rule*`/`rule*`), row axis entirely missing

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Тип:** ДОРАБОТКА — никогда не реализованная функциональность (правильная per-axis модель и `<gap-rule-list>` грамматика), не дефект уже работающего кода
**Дата:** 2026-08-04
**Компонент:** css-parser/layout (`crates/engine/layout/src/style.rs:3552-3559,15586-15625`), paint (`crates/engine/paint/src/gap_decorations.rs`)
**Найден:** WPT-RUN-3 срез 37 (`ROADMAP.md`) — массовый прогон `css/css-gaps`

## Ревизия P3 2026-09-13

Переквалифицировано из бага в ДОРАБОТКУ (см. `feedback_feature_gap_is_not_a_bug`
в памяти проекта / прецеденты BUG-492/BUG-511/BUG-521/BUG-538): объём работы —
не переименование одного свойства, а (1) разбиение единого
axis-agnostic `gap_rule_*` триплета на два независимых
(`column_rule_*` в grid/flex-смысле + новый `row_rule_*`), не сталкиваясь
с одноимённым multicol-полем `column_rule_*`, уже занимающим это имя в
Rust, (2) регистрация `column-rule`/`row-rule`/`rule` (+ лонгхэнды) как
известных свойств для flex/grid-контейнеров без коллизии с multicol на
multicol-контейнерах, и (3) с нуля реализованная грамматика
`<gap-rule-list>`/`<gap-auto-rule-list>` (`repeat()`, посегментные списки,
`outset`/`inset`/`overlap-join`/`cap`) — этой грамматики в парсере нет вообще,
не «недопарсена». Тот же класс, что закрытые ранее BUG-521/538: новый
алгоритм, а не точечный патч. `CSS-SPECS.md:113` уже понижен до 🟡 с точным
описанием остатка и ссылкой на этот баг — отдельная строка в `ROADMAP.md` не
нужна (путь «CSS-свойство → CSS-SPECS.md» задачу не заводит, см. BUG-538).
Указатель убран из `STATUS-P3.md`.

## Механизм

`CSS-SPECS.md:113` marks "CSS Gap Decorations L1" ✅ done, citing a
`gap-rule-width`/`gap-rule-style`/`gap-rule-color` shorthand+longhand group
wired into flex/grid/multicol gap painting (`style.rs:3552-3559` fields,
parsed at `style.rs:15586-15625`, rendered by
`paint::gap_decorations::emit_gap_rules`). Those property names do not exist
in the shipped spec (<https://www.w3.org/TR/css-gaps-1/>): the actual surface
is **per-axis** — `column-rule`/`column-rule-style`/`column-rule-color`/
`column-rule-width` for the column axis, the sibling `row-rule*` group for
the row axis, and a `rule`/`rule-style`/`rule-color`/`rule-width` shorthand
group that sets both axes at once. `gap-rule*` was apparently an earlier
draft name that never made it into the current TR, and the implementation
was never renamed to track the spec's naming change.

Consequences, both confirmed by grep:
- `crates/engine/css-parser/src/lib.rs:121-124` only registers the
  pre-existing CSS Multi-column `column-rule*` longhands (multicol's own
  rule-between-columns feature, spec'd separately and already correct for
  multicol); there is no `row-rule*`/`rule*` registration at all, so those
  identifiers are unknown properties end to end.
- `style.rs` carries exactly one axis-agnostic rule triplet
  (`gap_rule_width`/`_style`/`_color`, non-inherited) — one rendering style
  for both column and row gaps, not two independent ones — so even a
  find-and-rename from `gap-rule*` to spec names could not fully close this
  without splitting the field into `column_rule_*` (grid/flex sense,
  distinct from the multicol `column_rule_*` triplet already occupying that
  Rust name) and a new `row_rule_*` triplet.
- The parser's `gap-rule` shorthand only understands `<line-width> ||
  <line-style> || <line-color>` — none of the spec's `<gap-rule-list>` /
  `<gap-auto-rule-list>` grammar (`repeat(auto, ...)`, `repeat(<integer>,
  ...)`, per-segment lists, `outset`/`inset`/`overlap-join`/`cap`
  behavior-at-intersections keywords) is parsed.

Net effect: every WPT test that sets `column-rule`/`row-rule`/`rule` (or any
longhand) on a flex/grid container and then reads it back via inline style
(`el.style.columnRuleStyle`) or `getComputedStyle` sees the property as
completely unsupported — canonicalization/serialization tests fail with
`expected "10px" but got ""`, and `"<prop> in getComputedStyle(el)"`
feature-detects fail with `expected true got false`, because the underlying
CSS-parser property table has no entry to resolve.

## Симптом

`css/css-gaps` mass run (WPT-RUN-3 slice 37): 75/75 harness OK, 794/4148
subtests passed, 3354 failing. **3353 of those 3354** across `parsing/`,
`animation/`, and other subdirectories match this one root cause — e.g.
`gap-decorations-rule-shorthand.html`: `assert_true: column-rule-style
doesn't seem to be supported in the computed style expected true got
false`; `rule-width-interpolation-conversion-001.html`: `assert_equals:
expected "0px" but got ""`. The remaining 1 failure
(`gap-decorations-important.html`, `target is not defined`) is unrelated —
covered by the already-open BUG-384 (named access on `Window` missing).

## Масштаб находки

Dominant cluster of the whole `css-gaps` category (99.97% of its failing
subtests). Fixing requires: (1) renaming/splitting the ComputedStyle fields
into per-axis `column_rule_*`/`row_rule_*` triplets distinct from multicol's
existing `column_rule_*`, (2) registering `column-rule`/`row-rule`/`rule`
(+ longhands) as recognized properties for flex/grid containers without
colliding with multicol's identically-named `column-rule*` on multicol
containers, (3) implementing the `<gap-rule-list>`/`<gap-auto-rule-list>`
value grammar (`repeat()`, per-segment override lists, intersection-behavior
keywords) in the parser, and (4) re-verifying `CAPABILITIES.md`/
`CSS-SPECS.md:113`, which currently claims this module is done.

## Срез 1 (P4, 2026-10-03, p4-gap-rule-axes)

Закрыты пункты (1), (2) и имена свойств из (4): нестандартный `gap-rule*` удалён;
`column-rule*` (общие `column_rule_*` с multicol) рисуют вертикальные сегменты flex/grid,
новые `row_rule_*` + `row-rule*` — горизонтальные, `rule*` задаёт обе оси. Шортхенд парсит
`<line-width> || <line-style> || <color>` (`thin/medium/thick`, дубль/мусор → декларация
отброшена), initial ширины — `medium` (3px). Остаток — пункт (3): `<gap-rule-list>`/
`repeat()`, `*-rule-inset`/`-break`/`-overlap`/`-visibility-items`, интерполяция.

## Срез 2 (P4, 2026-10-03, p4-gap-rule-visibility)

Разобраны, каскадируются (non-inherited, CSS-wide, `getComputedStyle`) `column-rule-break`/`row-rule-break`/`rule-break`, `*-rule-visibility-items`/`rule-visibility-items` и `rule-overlap`; `rule-overlap` уже задаёт порядок рисования осей. `*-rule-break` и `*-rule-visibility-items` хранятся, но paint их не читает — нужна геометрия сегментов по таблицам треков grid (`col_offsets`/`row_offsets` в `grid_trampoline.rs`), сейчас `collect_gap_segments` восстанавливает щели по краям детей. Остаток: `<gap-rule-list>`/`repeat()`, `*-rule-inset*`, paint для break/visibility, интерполяция.

## Срез 3 (P4, 2026-10-03, p4-gap-rule-inset)

Разобраны, каскадируются (non-inherited, CSS-wide, `getComputedStyle`) все `*-rule-inset*`: восемь longhand-ов `{column,row}-rule-inset-{cap,junction}-{start,end}`, шортхенды `-start`/`-end`/`-cap`/`-junction`, `{column,row}-rule-inset` (`cap-start cap-end? [/ junction-start junction-end?]?`) и `rule-inset*` (обе оси); значение `<length-percentage> | overlap-join`. Paint применяет **cap**-вставки: отрезки идут на всю длину контейнера, их концы — края контейнера, где ширина пересекающей щели 0 (`%` и `overlap-join` → 0); ось строк зеркалится при `direction: rtl`. **Junction**-вставки хранятся, но не читаются — нужны посегментные разрывы (`*-rule-break`) по таблицам треков. Остаток: `<gap-rule-list>`/`repeat()`, paint для break/visibility/junction-inset, интерполяция.

## Срез 4 (P4, 2026-10-03, p4-gap-rule-list)

Пункт (3) закрыт по грамматике: `*-rule-width/-style/-color` и шортхенды `column-rule`/`row-rule`/`rule` принимают `<gap-rule-list>`/`<gap-auto-rule-list>` — значения через запятую, `repeat(<integer [1,∞]>, …)`, не более одного `repeat(auto, …)`; невалидный список (пустой элемент, `repeat(0, …)`, два `auto`, вложенный `repeat()`) отбрасывает декларацию. Поля `{column,row}_rule_{width,style,color}` стали `RuleList<T>` (`style/values/rule_list.rs`), computed value хранит исходную форму с `repeat()`. Paint раздаёт значения щелям оси по §4.6 (flex/grid — `gap_decoration_commands`, multicol — `emit_column_rules`); элемент `none`/нулевой ширины пропускает свою щель, но расходует значение. Шортхенд `*-rule` в `getComputedStyle` сериализуется только для списков без `repeat()` одинаковой длины. Остаток: paint для break/visibility/junction-inset, интерполяция (§4.7), сквозная нумерация щелей по схлопнутым/фрагментированным желобам и строкам flex-wrap (сейчас щели нумеруются по оси в порядке координат).

## Срез 5 (P4, 2026-10-03, p4-gap-rule-break)

Paint grid-контейнера читает `*-rule-break`, `*-rule-visibility-items` и junction-вставки. `grid_gap_segments()` (`paint/src/gap_decorations.rs`) восстанавливает дорожки по прямоугольникам детей (щель — правое ребро элемента → левое ребро другого через `gap`), переводит каждого ребёнка в диапазон дорожек и режет щель на куски (`grid_gap_pieces()`, §3.1.2): `none` — одна линия от края до края; `normal` — разрыв на «Т»-стыках, сквозь «крест» линия идёт; `intersection` — разрыв на любом стыке; кроме `none`, линия также обрывается там, где щель пересекает элемент. `all|around|between` прячут куски у пустых клеток (`normal` = `all`). Вставки: `cap-*` на краях контейнера, `junction-*` в точках разреза (`%` от ширины пересекающей щели, `overlap-join` = полщели + полширины линии пересечения). Куски одной щели делят её номер в списке значений. Ограничения: щель не находится, если рядом нет пары упирающихся в неё элементов (пустая дорожка целиком, нестретчнутые элементы); flex-wrap/multicol/subgrid/grid-lanes break/visibility не читают. Остаток: интерполяция (§4.7), сквозная нумерация щелей по схлопнутым/фрагментированным желобам.
