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

Paint flex-контейнера читает `*-rule-break: intersection` и inset-вставки (p4-gap-rule-flex, 2026-10-03). `flex_gap_segments()` (`paint/src/flex_gap_decorations.rs`) группирует детей во flex-строки: щель главной оси — отрезок своей строки (щели соседних строк не сливаются), щель поперечной оси лежит между строками и при `intersection` режется в стыках с главными щелями обеих соседних строк; `cap-*` — на краях контейнера, `junction-*` — в стыках. Значения `<gap-rule-list>` раздаются по порядку размещения сквозь все строки (§4.6). Остаток: интерполяция (§4.7), `writing-mode` ≠ `horizontal-tb`, сквозная нумерация по схлопнутым/фрагментированным желобам, subgrid/grid-lanes.

## Срез 7 (P4, 2026-10-03, p4-gap-rule-interp)

Интерполяция §4.7 в Web Animations: `*-rule-width`/`-color`/`-inset-*` (`style/values/rule_interp.rs`, нативный `_lumen_css_interpolate_gap_rule`, ветка `_wa_gap_prop_re` в `_wa_interp_prop` и неявные ключевые кадры в `_wa_compute_at_p`). Списки без `auto` — раскрытие `repeat()` и НОК длин; со `auto` — только при равных ведущих/хвостовых частях, форма `repeat(auto, …)` сохраняется; иначе перелом на 50%. Ширина привязывается как border-width, цвет — premultiplied sRGB, `%` в inset даёт `calc()`. `css-gaps/animation`: 490 → 886 из 2396 подтестов, все 12 одиночных `neutral-keyframe` зелёные, Web Animations 578/581. **Остаток:** CSS Transitions и CSS `@keyframes` для этих свойств (`TransitionScheduler`/`AnimationScheduler` знают только opacity/color/background-color/transform/height; ~1070 подтестов + 4 `rule-*-conversion-*`), `-0%` в `calc()` при `10px → -10%` (2 подтеста), `repeat(auto, …)` при `t=1.5` с переполнением (1), сквозная нумерация щелей.

## Срез 8 (P4, 2026-10-03, p4-gap-rule-transitions)

CSS Transitions для `*-rule-width`/`-color`/`-inset-*` как переход **вычисленного значения** в JS-шиме: `_wa_gap_tr_value` (`web_api_shim_tail_b.js`) подключён к `getComputedStyle().getPropertyValue`. Первое чтение запоминает установившееся значение, чтение с другим значением запускает переход, если списки `transition-property` (лонгхенд, шортхенды `rule-*`/`column-rule-*`/`row-rule-*`/`*-inset-*`, `all`), `-duration`, `-delay`, `-timing-function` его просят; нулевая длительность и неинтерполируемая пара перехода не дают; смена цели посреди перехода стартует от текущего значения. Арифметика — та же `rule_interp.rs`; `currentcolor` разворачивается в `color` до интерполяции, прогресс округляется до 1e-5 (f32-сериализация `cubic-bezier` иначе ломала `floor` ширины). `css-gaps/animation`: 886 → 1816 из 2396 подтестов, Web Animations 578/581, CSS Transitions 526/581. Метаданные `.ini` перегенерированы (`--update-expected`).

**Остаток:** CSS `@keyframes` для этих свойств (`AnimationScheduler`, 543 подтеста); переходы `repeat(auto, …)` с `transition-behavior: allow-discrete` (24) и «mid-animation» смена computed value (4); `-0%` в `calc()` при `10px → -10%` (3 подтеста: WPT ждёт `calc(-0% + 10px)` для `cap-end` и `calc(0% + 10px)` для `cap-start`/`junction-*` — одно и то же значение, разный ожидаемый знак; это вопрос Chrome-специфики, не арифметики); `repeat(auto, …)` при `t=1.5` (1). Переход виден только через `getComputedStyle()` — линии не перерисовываются покадрово и события `transitionrun/start/end` для них не приходят (Rust `TransitionScheduler` их не знает).

## Срез 9 (P4, 2026-10-03, p4-gap-rule-keyframes)

CSS `@keyframes`-анимации `*-rule-width`/`-color`/`-inset-*` как анимация **вычисленного значения** в JS-шиме рядом с переходами: `_wa_gap_an_value` (`web_api_shim_tail_b.js`) подключён к `getComputedStyle().getPropertyValue` поверх `_wa_gap_tr_value`. Списки `animation-name/-duration/-delay/-timing-function/-iteration-count/-direction/-fill-mode/-play-state` берутся из вычисленного стиля, ключевые кадры — нативом `_lumen_keyframes_json` (`install/stylesheets.rs`, последнее `@keyframes` с этим именем по включённым листам). Часы анимации стартуют на первом чтении, которое её видит (одно `performance.now()` на задачу), пауза замораживает прогресс; прогресс — задержка, итерации, `direction`, `fill-mode`; отсутствующий кадр 0%/100% — нейтральный (установившееся значение); `rule-width`/`rule-color` в кадре задают обе оси; последняя анимация списка побеждает. Арифметика — та же `rule_interp.rs`. `css-gaps/animation`: 1816 → 2342 из 2396 подтестов, CSS Animations 564/581, Web Animations 578/581, `row-rule-inset-interpolation` больше не TIMEOUT. Метаданные `.ini` перегенерированы.

**Остаток:** переходы `repeat(auto, …)` с `transition-behavior: allow-discrete` (24) и «mid-animation» (4; два из них ждут `el.getAnimations()[0]` для CSS-анимации); неинтерполируемые пары в анимации перелом на 50% отдаёт сырое значение (`red` вместо `rgb(255, 0, 0)`, `repeat(auto, …)` при `t=1.5` — 14 подтестов `column-rule-color`); `-0%` в `calc()` (2); кадры-шортхенды (`column-rule: …` внутри `@keyframes`) не читаются, `animation-composition` не учитывается; линии не перерисовываются покадрово, событий `animation*` нет, `getAnimations()` CSS-анимацию этих свойств не отдаёт (Rust `AnimationScheduler` их не знает).


## Срез 10 (P4, 2026-10-03, p4-gap-rule-animation-edges)

`transition-behavior: normal | allow-discrete` (CSS Transitions L2 §3.1) разобран как свойство: список-лонгхенд (невалидный элемент отбрасывает декларацию), пятый компонент шортхенда `transition` (слой без него сбрасывается в `normal`), `ComputedStyle::transition_behaviors`, `getComputedStyle`, `CSS.supports`. Переход вычисленного значения `*-rule-*` с неинтерполируемой парой (разные формы `repeat(auto, …)`, `overlap-join`) стартует только с `allow-discrete` и переламывается на 50% (`_wa_gap_tr_value`). Дискретно переломленное значение (переход, `@keyframes`, Web Animations) отдаётся в computed-форме — `red` → `rgb(255, 0, 0)`, форма `repeat()` сохранена (нативный `_lumen_css_canonical_gap_rule` → `canonical_gap_rule_value` в `rule_interp.rs`). `css-gaps/animation`: 2342 → 2380 из 2396 подтестов; `rule-width-interpolation-repeaters.html` полностью зелёный (`.ini` удалён), в `rule-color-interpolation-repeaters-001.html` осталось 4.

**Остаток (16 подтестов):** `calc(-0% + 10px)` против `calc(0% + 10px)` для `cap-end` (8: WPT ждёт знаковый ноль Chrome, для `cap-start`/`junction-*` — беззнаковый, арифметика одна); `repeat(auto, …)` при `t=1.5` (4: ожидаемое значение в тесте — `repeat(auto, rgb(0, 0, 255), rgb(255, 0, 0)` с незакрытой скобкой; по CSS Syntax §5.4.7 она закрывается в конце значения, а `RuleList::parse` такую строку отвергает, и `style.setProperty` оставляет прежнее `repeat(2, …)`; закрытие скобки в `RuleList::parse` проверено и откачено — оно ломает существующий кейс `repeat(2, 1` в `invalid_lists_are_rejected`, который объявляет такую строку невалидной; нужно решение, какой из двух источников прав); «mid-animation» (4: нужен `el.getAnimations()[0]` для CSS-анимации этих свойств). Painted-переходы/анимации и события — по-прежнему не сделаны.

## Срез 11 (P4, 2026-10-03, p4-gap-rule-painted)

Покадровое рисование переходов и `@keyframes` для `*-rule-width`/`*-rule-color`: `TransitionScheduler::sync_gap_rules` и `shell::AnimationScheduler` (`interpolate_gap_rules`) кладут `GapRuleOverride` (`style/values/rule_anim.rs`) в `AnimatedStyle::gap_rules` → `CompositorOverride::gap_rules`; `gap_decoration_commands(b, gap_rules)` рисует щели flex/grid из перекрытых значений без relayout (ordered и `walk_with_anim`). События `transitionrun/start/end/cancel` этих свойств идут от планировщика. Тайминги берутся по последнему токену `transition-property`, покрывающему свойство (`all`, `rule`, `column-rule`, `rule-width`, лонгхенд); не интерполируемая пара — только с `allow-discrete`; прерванный переход продолжается с текущего значения; отсутствующий 0%/100% keyframe — вычисленное значение элемента.

**Остаток:** `*-rule-inset-*` и multicol не рисуются покадрово; `animationstart/iteration/end` и `getAnimations()` для `@keyframes` этих свойств; нумерация щелей по схлопнутым/фрагментированным желобам и строкам flex-wrap.


## Срез 12 (P4, 2026-10-03, p4-gap-rule-getanimations)

`el.getAnimations()` отдаёт `@keyframes`-анимацию `*-rule-*`: `_wa_gap_an_sync` (`web_api_shim_tail_b.js`, зовётся из `_wa_get_animations_for`) заводит `Animation` под тем же ключом реестра `a:`, что и `animationstart` планировщика, кладёт в эффект вычисленный тайминг `animation-*` (`getComputedTiming().duration`), а `currentTime` читает и сдвигает ту же запись часов, по которой `getComputedStyle()` считает значение — перемотка видна следующему чтению. Завершённая анимация без `forwards`/`both` из списка уходит. `css-gaps/animation`: 2380 → 2384 из 2396; четыре `rule-{color,width}-interpolation-conversion-00{1,2}.html` зелёные (`.ini` удалены). Юнит-тесты — `v8_gap_rule_interp.rs`.

**Остаток (12 подтестов):** `-0%` в `calc()` (8), `repeat(auto, …)` при `t=1.5` с незакрытой скобкой (4; решение по `RuleList::parse` ждёт ответа). Вне `css-gaps`: `getAnimations()` не возвращает `CSSAnimation` для остальных свойств (BUG-536).

## Срез 13 (P4, 2026-10-03, p4-gap-rule-eof-paren)

Решение по незакрытой скобке: права CSS Syntax §5.4.7 (конец значения закрывает открытые функции), а не тест `repeat(2, 1` в `invalid_lists_are_rejected` — он был написан до WPT и переведён на `repeat(2, 1))` (лишняя `)` по-прежнему невалидна). `RuleList::parse` дописывает недостающие `)` (`close_open_parens`), JS `CSSStyleDeclaration.prototype.setProperty` делает то же для `rule*`/`{column,row}-rule*` (`_lumen_close_open_parens`, `web_api_shim_mid.js`): раньше открытая `(` проглатывала `;` при сериализации атрибута `style`, и значение терялось при перечитывании. `rule-color-interpolation-repeaters-001.html` полностью зелёный (`.ini` удалён), `css-gaps/animation`: 2384 → 2388 из 2396.

**Остаток (8 подтестов):** `-0%` в `calc()` для `cap-end` (WPT ждёт знаковый ноль Chrome, для `cap-start`/`junction-*` — беззнаковый, арифметика одна).
