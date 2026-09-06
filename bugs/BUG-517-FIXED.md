# BUG-517: CSS Rhythm `block-step-size`/`-insert`/`-align`/`-round` not implemented at all

**Статус:** FIXED 2026-09-06 (P3)
**Дата:** 2026-08-03
**Компонент:** css-parser + layout (`grep -n "\"block-step" crates/engine/css-parser/src/lib.rs
crates/engine/layout/src/style.rs` — zero hits. `line-height-step`, the
module's other property, is implemented (✅ in CSS-SPECS.md) — this is the
module's second, unrelated property family, not a partial gap in the same
one.)
**Найден:** WPT-RUN-3 срез 22 (`ROADMAP.md`) — массовый прогон `css/css-rhythm`

## Симптом

```
FAIL Property block-step-align value 'auto'
  assert_true: block-step-align doesn't seem to be supported in the
  computed style expected true got false
FAIL e.style['block-step'] = "auto" should set the property value
  assert_equals: serialization should be canonical expected "none" but
  got "auto"
```

## Механизм

`block-step`/`block-step-size`/`block-step-insert`/`block-step-align`/
`block-step-round` (CSS Rhythmic Sizing L1 §3, an Editor's Draft) are not in
the parser's known-property table at all — no parsing, no `ComputedStyle`
storage, no cascade/inheritance entry. The `e.style[...] = "..."` failures
are not a distinct gap: because the property name is unrecognized, the
generic inline-style-setter passthrough ([BUG-484](BUG-484-OPEN.md)) takes
over and stores the raw string verbatim instead of parsing+canonicalizing
it — same downstream shape as every other unimplemented-property category
in this track.

## Масштаб находки

15 files / 155 subtests, all under `css/css-rhythm/parsing/` — the entire
category (100% of its testharness ids). File per longhand:
`block-step-computed.html` (26), `block-step-valid.html` (23),
`block-step-align-invalid.html`/`-insert-invalid.html`/`-round-invalid.html`
(12/12/9 — BUG-484's setter passthrough), `block-step-size-valid.html` (1,
canonical-serialization form of the same passthrough), plus the remaining
`-align`/`-insert`/`-round`/`-size` computed/invalid files split the same
way. 0 files reach a genuinely distinct failure mode — filed as a single
bug per the module, not five, since fixing "recognize block-step* as
properties" fixes all five longhands' non-invalid subtests identically.

## Что нужно

Add `block-step`/`block-step-size`/`block-step-insert`/`block-step-align`/
`block-step-round` to the parser's known-property table (`css-parser/src/
lib.rs`), matching `ComputedStyle` fields, cascade/inheritance wiring, and
`computed_style_to_map` entries (per [BUG-472](BUG-472-OPEN.md)'s pattern).
Filed per the standing track policy of filing even very-early-draft specs
(precedent: [BUG-507](BUG-507-OPEN.md) `css-exclusions`) — Editor's Draft,
not yet shipped in any evergreen browser, low implementation priority.

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-rhythm/` for all 15
files, `expected: FAIL` per subtest.

---

## Резолюция (2026-09-06, P3)

**Ревизия премисы:** несмотря на "low implementation priority" в исходной
заявке, CSS Rhythmic Sizing остаётся активно редактируемым W3C Working
Draft (последняя публикация 2026-02-17, `w3.org/TR/css-rhythm-1/`), а не
мёртвым модулем вроде `css-exclusions`(BUG-507)/`css-link-params`(BUG-511) —
реклассификация в ДОРАБОТКА неуместна, это обычная точечная правка добавления
свойства.

**Реализовано** (Phase 0 — parse + store + cascade + CSSOM, без layout-
эффекта, тот же класс, что уже принятый в модуль `line-height-step`):

- `block-step-size`/`-insert`/`-align`/`-round` добавлены в
  `SUPPORTED_PROPERTIES` (`crates/engine/css-parser/src/lib.rs`).
- Три новых enum'а `BlockStepInsert`/`BlockStepAlign`/`BlockStepRound`
  (`style/values/misc.rs`) + поле `block_step_size: Option<f32>`
  (`style/computed.rs`) — `None` = `none` (initial), `Some(px)` — резолвлено
  eagerly через `resolve_block_step_size` (`style/values/length.rs`,
  переиспользует `<length [0,∞]>`-грамматику `overflow-clip-margin`: без
  `%`, литеральный отрицательный отклоняется, отрицательный результат
  `calc()` зажимается до 0).
- Парсинг четырёх лонгхендов + шортхенда `block-step` в
  `style/apply/layout.rs` (`apply_block_step_shorthand`) — грамматики всех
  четырёх слотов не пересекаются ни одним токеном, поэтому каждый
  whitespace-токен однозначно относится к одному слоту; дубликат слота или
  нераспознанный токен инвалидирует всё объявление целиком (CSS Cascade
  L4 §7).
- CSS-wide keyword (`style/apply/css_wide.rs`) — ни одно из пяти свойств не
  наследуется.
- `computed_style_to_map` (`selector_query.rs`) — пять новых записей,
  включая каноническую сериализацию шортхенда
  (`block_step_shorthand_computed`: `none` при всех значениях по
  умолчанию, иначе non-initial значения в порядке `size insert align
  round`).
- CSSOM `element.style` (JS-шим, `web_api_shim_mid.js`): новый нативный
  `_lumen_css_canonical_block_step_size` (аналог
  `_lumen_css_canonical_overflow_clip_margin`, но без box-keyword),
  `block-step-insert`/`-align`/`-round` — записи в `_LUMEN_KEYWORD_
  PROPERTIES` (переиспользуют существующий generic keyword canonicalizer),
  и полноценный expand/collapse шортхенда `block-step`
  (`_lumen_expand_block_step_shorthand`/`_lumen_block_step_shorthand_value`),
  подключённый и к `_lumen_parse_style` (атрибут `style="…"`), и к
  `setProperty`/`getPropertyValue`.

**Живой WPT-прогон** (`tests/wpt/run_report.py --root css/css-rhythm
--recursive`, `dev-release`): 127/155 сабтестов `parsing/` теперь проходят
(было 0/155 не считая структурных фейлов-по-умолчанию); `.ini` обновлены
`--update-expected`, `--check` — 0 regression(s) по всей категории
(200 checks, 183 subtests, 17 tests).

**Остаток, вне скоупа этой правки:**
- 28 сабтестов `*-computed.html` для НЕ-дефолтных значений падают —
  тот же класс, что уже задокументирован для `text-size-adjust`
  (BUG-513)/BUG-514: `getComputedStyle()` в тесте читает состояние в одном
  синхронном тике скрипта до первого релейаута (класс BUG-493). Не
  специфично для `block-step*` — воспроизведено на уже принятом
  `text-size-adjust` тем же прогоном (`css/css-size-adjust/parsing/
  text-size-adjust-computed.html` — тот же паттерн: только initial-значение
  проходит).
- `computedstyle/{block-level-replaced-elements-affected,inline-level-
  replaced-elements-not-affected}-by-block-step-size.html` (28 сабтестов,
  реальный layout-эффект `block-step-size` на высоту блочных
  replaced-элементов) — были `expected: TIMEOUT` (вся заявка не
  распознавалась, харнесс не завершался), теперь харнесс завершается
  (`OK`), но сабтесты остаются `FAIL` — реальный алгоритм деления на шаги
  не реализован (Phase 0, тот же скоуп, что и у `line-height-step`).
  `.ini` обновлены `--update-expected`, чтобы `TIMEOUT` не маскировал
  дальнейший дрейф.

**Проверки:** `cargo test -p lumen-layout` — 13 новых юнит-тестов
(`style/tests/block_step_tests.rs`), 3838/3838 в крейте зелёные;
`cargo clippy -p lumen-layout -p lumen-css-parser -p lumen-js --all-targets
--features v8-backend -- -D warnings` чисто.
