# BUG-519: `@function` bodies using `if()`/`attr(type())`/local vars/nested `@layer`/`@container`/shadow scoping hang instead of failing

**Статус:** FIXED 2026-09-24 (P3)
**Дата:** 2026-08-03
**Компонент:** css-parser/layout, exact trigger not isolated (see Механизм)
**Найден:** WPT-RUN-3 срез 22 (`ROADMAP.md`) — массовый прогон `css/css-mixins`

## Симптом

wptrunner's own per-test timeout (~10s) fires with **zero** subtests
registered, instead of the graceful `NNN is not defined`/`assert_equals`
`FAIL` every other `@function`-testing file in the same directory produces:

```
2:03.12 TEST_END: TIMEOUT, expected OK
Subtests passed 0/0
```

Confirmed as a real wall-clock hang, not a harness artifact: measured
directly off the raw wptrunner log, `function-layer.html` alone ran
`TEST_START`→`TEST_END` in **71 seconds** (0:51.94 → 2:03.12) — far past
the point a normally-failing test reports (typically well under 1s).

## Механизм

Not isolated to a single cause — each of the 9 affected files exercises a
different combination of `@function`-adjacent constructs that are all
individually documented as deferred/unimplemented in CSS-SPECS.md's T3 row
(`@function`: 🟡, "`returns` typing + conditional group rules deferred"):
`if()` (CSS Values L5 conditional function, inside a `result:` expression),
`attr(data-x type(*))`/`attr(data-x type(<length>))` (typed `attr()`),
locally-scoped `--x:`-style declarations inside a `@function` body read
back via `var()`, `@function` nested inside `@layer`/`@container`, and
`@function` invoked from inside a shadow tree. Every other file in the same
directory that touches one of these fails *fast* with a normal `FAIL`
(e.g. `function-container-dynamic.html`, `target is not defined` —
[BUG-384](BUG-384-FIXED.md)) — only the 9 files below, all combining one of
these constructs with the *body* of a `@function` declaration itself (not
just referencing the result from outside), hang. Root cause not isolated
further this session (would need a live `--mcp-live-port` step-through or a
minimal single-construct repro per candidate — `if()` alone, `attr()`
alone, local-var alone, `@layer`-nesting alone — to narrow which of the
five is the actual trigger; plausibly more than one).

## Масштаб находки

9 files, all TIMEOUT with 0/0 subtests registered (0 subtests attempted,
not 0 passed): `function-conditionals.html`, `function-layer.html`,
`function-parameter-types.tentative.html`, `function-shadow-container.html`,
`function-shadow.html`, `local-attr-substitution.html`,
`local-if-substitution.html`, `local-inherit-substitution.html`,
`local-var-substitution.html`.

## Что нужно

Isolate which single construct (`if()`, typed `attr()`, `@function` local
vars, `@function`-in-`@layer`/`@container`, or `@function` invoked in a
shadow tree) causes the hang, via a minimal live-window repro per
candidate. Given `@function`'s "conditional group rules"/`returns`-typing
gaps are already tracked as deferred scope (CSS-SPECS.md T3), the fix
priority here is specifically "don't hang" (fail fast/gracefully on the
unsupported construct), independent of whether full `@function` L1
compliance is implemented.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-mixins/functions/` for
all 9 files, `expected: TIMEOUT`.

## Срез P3 2026-09-06

Investigated all five candidate constructs directly (unit-level `cascade_at`/
`compute_style` probes with a wall-clock assertion, plus a `crates/driver`
`InProcessSession::eval` probe replicating the WPT harness's own dynamic
`<style>`-element insertion) — **no infinite loop reproduces today** in any
of: `@function` nested inside `@layer`, `@supports`/`@media`/`@container`
nested inside a `@function` body (including 3-deep combinations), `if()`
inside `result:`, typed `attr()` (including a self-referential `attr(data-x)`
whose own DOM attribute text is `var(--x)`), `inherit()`, typed parameters,
and `@function` declared/called from inside a shadow tree. Every probe
completed in microseconds. `tests/wpt/run_smoke.py` remains broken in this
environment (Python 3.14's `ssl.wrap_socket` removal, unrelated to this bug —
see project memory), so a live wptrunner re-run to directly disprove the
original 71-second `TEST_END` was not possible; the conclusion rests on these
probes plus the two real defects found and fixed below, either of which is
sufficient to explain **wrong output**, though neither reproduces an actual
hang.

**Found and fixed — two real parser correctness bugs, independent of each
other, each matching part of the original symptom list:**

1. **`recover_to_decl_boundary` (`crates/engine/css-parser/src/parser/
   declarations.rs`) was not brace-depth-aware.** `parse_declaration_block`
   (used for a `@function`/`@mixin` body — plain declarations only, no
   at-rule grammar) falls back to this recovery function whenever it meets
   something that isn't a `property: value;` declaration, e.g. a nested
   `@supports`/`@media`/`@container` block (`function-conditionals.html`,
   `function-shadow-container.html`). The old recovery scanned for the next
   `;` or `}` with no brace tracking at all: it stopped at the nested
   block's own *first* `;` (e.g. inside `@supports (...) { --unused: 1; }`),
   then treated the nested block's own closing `}` as the end of the
   *entire* `@function` body — silently dropping `result:` (and, in a real
   multi-rule stylesheet, everything after it) without any error. Confirmed
   directly: before the fix, every nested-`@supports`/`@media`/`@container`
   probe case left `--actual` completely unset (not merely unresolved); after
   the fix, the declaration survives. Fixed by having the recovery detect a
   `{` and delegate to the existing (already correct) `skip_block()` helper,
   then stop — leaving whatever legitimately follows the block for the
   caller's own loop.
2. **`parse_value_until_terminator` did not track paren depth.** CSS Values
   L5's `if(<condition>: <value>; else: <value>;)` uses `;` *inside* its own
   parens to separate branches — CSS Syntax L3 §5.4.4 only ends a
   declaration's value at a *top-level* `;`/`}`. The old code stopped at the
   first `;` regardless of nesting, so `result: if(style(--x: 3px): PASS;
   else: FAIL;);` was truncated mid-`if()`, and the orphaned `else: FAIL;)`
   tail was misparsed as a bogus second declaration — corrupting whatever
   followed in the same block (`local-if-substitution.html`,
   `function-parameter-types.tentative.html`, and `local-if-substitution`'s
   sibling `if()`-in-condition forms). Fixed by tracking `(`/`[` nesting and
   only treating `;`/`}` as a terminator at depth 0.

Both are narrowly scoped, single-call-site changes (`parse_value_until_
terminator` and `recover_to_decl_boundary` each have exactly one caller) with
no behavioural change for any well-formed value/declaration — matched-paren
values (`calc()`, `rgb()`, existing `var()`/`attr()` calls) never contained a
raw `;`/`}` inside their parens before, so the depth tracking only changes
outcomes for the previously-mishandled cases. 4 new permanent regression
tests: 3 in `crates/engine/css-parser/src/parser/tests/nesting.rs`
(structural, transcribing the exact shapes above) and 2 in
`crates/engine/layout/src/style/tests/values.rs` (through the real cascade).
`cargo test -p lumen-css-parser --lib`: 411/411 (was 407, +4). `cargo test -p
lumen-layout --lib`: 3879/3879 (unchanged count — the two new layout tests
replace two probe throwaways, net zero). `cargo clippy -p lumen-css-parser -p
lumen-layout --all-targets -- -D warnings`: clean. `graphic_tests/
dump_golden.py --build`: same pre-existing 4/12 mismatches (`samples/
page.html`, `65-flex-align-content.html`) as every other slice on the
adjacent BUG-518 track this same day — confirmed byte-identical on a clean
`main` checkout via `git stash` A/B (the [BUG-1008](BUG-1008-OPEN.md)-class
drift), unrelated to this change (declaration/value parsing only, no
paint/layout-geometry code touched). No live WPT run (`tests/wpt/
run_smoke.py` broken in this environment, unrelated to this bug).

**Found but NOT fixed this slice — filed separately as
[BUG-1010](BUG-1010-FIXED.md):** a custom property's own computed value never
resolves `attr()`/`--fn()`/`@apply`, only `var()`/`env()` — confirmed at both
the `ComputedStyle` level and through the actual `getComputedStyle()` JS
channel. Since all 9 files here (and the entire vendored `css-mixins`
category) observe results exclusively through `--actual`/`--expected` custom
properties (`tests/wpt/css/css-mixins/resources/utils.js::
test_all_templates`), this is very likely why these files would still not go
green even with the two fixes above and even if the original hang is gone —
they'd now fail fast with a value mismatch instead of hanging, which is
already a strict improvement per this bug's own stated priority ("don't
hang" independent of full compliance), but not a full fix. BUG-1010 is the
right place for that follow-up, not this bug.

Status remains `OPEN`: the literal hang could not be reproduced or
positively disproven in this environment (no live wptrunner), and even after
these fixes the 9 files are expected to fail (not hang) until BUG-1010 is
also fixed. Re-triage (`.ini` update from `TIMEOUT`/`FAIL` to whatever a live
run actually shows) needs a working `tests/wpt/run_smoke.py` first.

## Закрытие P3 2026-09-24

Живой wptrunner в этом окружении работает (`tests/wpt/.venv`, `run_smoke.py`
с бинарём `dev-release`) — прежнее «сломан Python 3.14» больше не
воспроизводится. Прогон всех 9 файлов на `main` до правки: **ни одного
TIMEOUT**, каждый файл `TEST_END` через ~4 с, `Test OK`. Зависание снято
фиксами среза 2026-09-06 и [BUG-1010](BUG-1010-FIXED.md); 7 из 9 файлов уже
проходили полностью (22/22, 7/7, 7/7, 7/7, 20/20, 5/5, 4/4).

Оставшиеся падения — один собственный дефект: **имена `@function` не были
tree-scoped** (CSS Scoping L1 §3.5, CSS Mixins L1 §2.4). Каскад звал
`expand_custom_functions` всегда с `sheet.function_rules` документа, даже для
декларации из `<style>` shadow-дерева — функции самого дерева были невидимы
(`function-shadow.html`: `--a()` давал документные `42px` вместо своих
`10px`, `--b()`/`--c()` оставались нераскрытыми).

Фикс:

* `expand_custom_functions_scoped` (`style/substitute.rs`) — принимает
  цепочку областей, внутреннее дерево первым. Вызов связывается с первой
  областью, где имя определено; тело функции резолвит свои вложенные вызовы
  начиная с области **своего определения** (`scopes[i..]`), а аргументы — в
  области вызывающего. Поэтому внешняя функция не видит одноимённую
  внутреннюю, а «одинаковое имя в разных деревьях» не считается циклом.
  Прежняя `expand_custom_functions` — обёртка с одной областью.
* `shadow_function_chain` (`style/cascade.rs`) — для каждого shadow-листа,
  давшего декларацию (`own_shadow`/`host_shadow`/`interior_shadow`), строит
  цепочку `function_rules` по `enclosing_shadow_host`; каскад выбирает
  цепочку по `shadow_origin` декларации. Строится только если в цепочке есть
  хоть одна функция — страницы без shadow-`@function` не платят ничего.

Замер после правки (`run_smoke.py`, 13 файлов каталога):
`function-shadow.html` 2/11 → **11/11**; `function-shadow-cache.html` 1/1
(был `expected: FAIL`); `function-shadow-container.html` 2/3. Остаток —
`::slotted() can see inner named containers`: `@container` внутри тела
`@function` не вычисляется вовсе (отложенный объём строки `@function` в
CSS-SPECS.md, не этот баг) — зафиксирован в `.ini` как `expected: FAIL`.
Остальные 6 проверенных файлов без изменений, неожиданных результатов нет.
`.ini` восьми зелёных файлов из списка и `function-shadow-cache.html.ini`
удалены.

5 регресс-тестов в `crates/engine/layout/src/style/tests/shadow_dom_selectors.rs`
(транскрипция `function-shadow.html`); `cargo test -p lumen-layout --lib`
4053/4053, `cargo clippy -p lumen-layout --all-targets -- -D warnings` чист.
