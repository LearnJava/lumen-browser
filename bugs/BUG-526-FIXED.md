# BUG-526: `MediaQueryList.media` doesn't serialize per the Media Queries
serialization algorithm — it just echoes the raw input string verbatim

**Статус:** FIXED 2026-09-06 (P3)
**Дата:** 2026-08-03
**Компонент:** js (`crates/js/src/dom.rs:10537-10543` — `MediaQueryList` constructor)
**Найден:** WPT-RUN-3 срез 24 (`ROADMAP.md`) — массовый прогон `css/mediaqueries`

## Механизм

```js
function MediaQueryList(media) {
    ...
    this.media = String(media == null ? '' : media);   // dom.rs:10540
    ...
}
```

`window.matchMedia(query).media` is spec'd (Media Queries §Serializing a
media query list) to run the query through the parser and re-serialize the
result: normalize/collapse whitespace, replace every invalid media query in
a comma-separated list with the literal string `not all`, drop redundant
parens/spacing, and canonicalize units (e.g. a `resolution` given as `x`
serializes as `dppx`; `calc()` expressions are simplified numerically). The
shim instead stores the constructor argument as-is with no processing at
all — `media` is a pure echo of whatever string was passed to
`matchMedia()`.

## Симптом

Every case in `css/mediaqueries/match-media-parsing.html` that isn't a
already-canonical single valid query fails: whitespace isn't trimmed/
collapsed (`" foo "` → expected `"foo"`, got unchanged), invalid clauses in
a list aren't replaced with `not all` (`",,"` → expected `"not all, not
all, not all"`, got unchanged `",,"`), and `calc()` inside `resolution`
isn't simplified/unit-canonicalized (`calc(1x)` → expected
`calc(1dppx)`, got unchanged `calc(1x)`). 25 subtests in that one file, plus
`aspect-ratio-serialization.html` (`1/3` → expected `1 / 3`, spacing not
added) and `mq-escaped-serialization.html` (1 subtest, CSS escape
normalization) — 27 subtests total this slice.

## Фикс (не сделан)

Implement the serialization algorithm against the already-parsed
`MediaQuery`/`MediaQueryClause`/`MediaFeature` AST (`crates/engine/
css-parser/src/parser.rs`) rather than storing the raw string: add a
`Display`/`to_css_string` impl that walks the parsed clauses, emits `not
all` for any clause containing `MediaCondition::Unsupported`, and
canonicalizes each feature's value (unit, whitespace) on the way out. Wire
that string into `_lumen_match_media`'s Rust side and return it alongside
the boolean match result so the JS constructor can store the serialized
form instead of the raw input.

## .ini

Committed `.ini` for `match-media-parsing.html` (25 subtests) and
`aspect-ratio-serialization.html` (1 subtest). `mq-escaped-serialization.html`
was already attributed to the unrelated [BUG-384](BUG-384-OPEN.md) (named
access on `Window`) and untouched by this fix.

---

## Резолюция (2026-09-06, P3)

**Реализовано:** `MediaQuery::serialize`/`MediaQueryClause::serialize`/
`MediaFeature::serialize` (`crates/engine/css-parser/src/parser/media.rs`)
walk the already-parsed AST — empty query list → `""`, comma-joined clauses
→ `", "`, a clause with an empty or `Unsupported` condition list → literal
`not all`, an `and`-list of features → `" and "`-joined, `not`/`only`
prefixes preserved. `MediaQueryClause` gained an `only: bool` field (needed
only for round-tripping the serialized prefix, no matching effect — `only`
was already a no-op in `matches()`). `aspect-ratio`/`min-`/`max-aspect-ratio`
switched from a pre-divided `f32` ratio to `(f32, f32)` numerator/
denominator so `1/3` serializes as `1 / 3` instead of a decimal
approximation. Wired a new native `_lumen_serialize_media_query` (`v8_runtime/
install/platform.rs`) into `MediaQueryList`'s constructor
(`web_api_shim_mid_b.js`) — `.matches` still evaluates against the raw
input (matching semantics unchanged), `.media` stores the serialized form.

**Живой замер (аналитический, через `parse_media_query(...).serialize()`
на каждый WPT `test_parsing()` case, 25+1 сабтестов):**
- `match-media-parsing.html`: 11/25 подтестов теперь дают ожидаемое
  значение (пустая строка, `all`-варианты, whitespace/comma-list
  normalization, `not all`-замена для `,`/`,,`/`  ,  ,  `). Прежние
  подтесты, где `expected === query` дословно (identity), уже проходили
  под старым echo-поведением и в `.ini` не числились.
- `aspect-ratio-serialization.html`: 1/1 — весь файл теперь зелёный,
  `.ini` удалён.
- 7 подтестов на `resolution`/`calc()` (`(min-resolution: calc(1x))` и
  соседи) остаются FAIL — `resolution`/`min-/max-resolution` не заведена
  как `MediaFeature` вообще (ни единиц `dppx`/`dpi`/`dpcm`, ни арифметики
  `calc()`), отдельный и заметно больший пробел, не «доделка серилизации
  поверх уже готового AST», как предполагал исходный план фикса — заведён
  [BUG-1019](BUG-1019-FIXED.md).
- 7 подтестов на boolean-context `(color)`/незакрытые скобки/дефолтный
  `word`-скан, не останавливающийся на `)` (`"color)"` парсится как
  буквальный media-type `"color)"` вместо ошибки) остаются FAIL —
  hand-rolled `parse_media_clause`/`parse_media_feature` не покрывает эти
  три смежных случая CSS-синтаксиса, заведён [BUG-1020](BUG-1020-FIXED.md).
- `mq-escaped-serialization.html` не тронут (блокирован BUG-384, не про
  сериализацию медиа-запроса — тест читает `conditionText` `CSSMediaRule`,
  которого у `CSSMediaRule` вообще нет; сама заявка эту причину не видела).

**`.ini`:** `match-media-parsing.html.ini` — 11 строк удалены (сабтесты
теперь проходят), 14 остаются `expected: FAIL` с обновлённой атрибуцией на
BUG-1019/BUG-1020. `aspect-ratio-serialization.html.ini` удалён целиком.

**Проверки:** новые юнит-тесты в `crates/engine/css-parser/src/parser/
tests/at_rules.rs` (9) и `crates/js/src/dom/tests/v8_matchmedia.rs` (3);
`cargo clippy -p lumen-css-parser -p lumen-js --all-targets --features
v8-backend -- -D warnings` чисто.
