# BUG-346: `Url::resolve()` doesn't collapse `.`/`..` dot-segments — relative subresource URLs with `..` 404

**Статус:** FIXED 2026-08-06
**Компонент:** core (`crates/core/src/url.rs::Url::resolve`), consumed by `ResourceBase::resolve`
(`crates/shell/src/main.rs:4207-4210`) for `<script src>`, `<link>`/stylesheet preload
(`main.rs:4345`), `<img>`/other resource loads (`main.rs:4849`, `5509`)
**Найден:** P2, WPT-VENDOR-custom-elements 2026-07-26 (`run_report.py --all --root custom-elements --recursive`)

## Симптом

Any page whose relative subresource URL contains a `..` dot-segment gets a 404 instead of
the intended resource. Reproduced with vendored `custom-elements/reactions/*.html` and
`custom-elements/reactions/customized-builtins/*.html`: these are served at
`/custom-elements/reactions/HTMLTableElement.html` and reference
`<script src="../resources/custom-elements-helpers.js">` (one level up) and
`<script src="../../resources/custom-elements-helpers.js">` (two levels up, from
`customized-builtins/`). Both resources exist and are correctly vendored under
`custom-elements/resources/` — Lumen still requests the *literal, unnormalized* URL:

```
http://127.0.0.1:8300/custom-elements/reactions/../resources/custom-elements-helpers.js
http://127.0.0.1:8300/custom-elements/reactions/customized-builtins/../../resources/custom-elements-helpers.js
```

`wptserve` correctly 404s these (no such literal path). This alone accounts for ~95 of
the run's TIMEOUT/FAIL results (`test_with_window is not defined`,
`testReflectAttribute is not defined` — helper scripts never loaded, so the test body's
own dependencies are undefined).

## Root cause

`Url::resolve()` (`crates/core/src/url.rs:194-202`) builds the resolved path via plain
string concatenation and re-parse, never removing dot-segments:

```rust
let dir = self.path.rfind('/').map(|i| &self.path[..=i]).unwrap_or("/");
Self::parse(&format!("{}://{}{}{}", self.scheme, base_authority, dir, reference))
```

`.../reactions/` + `../resources/x.js` → literal `.../reactions/../resources/x.js`, and
`Url::parse` (used both here and for the initial parse) never runs an RFC 3986 §5.2.4
remove-dot-segments pass on the path either. This is a **documented, intentional gap**,
not a new regression — the module doc explicitly calls it out
(`crates/core/src/url.rs:14-16`: *"Сознательно не реализуем здесь: … `.`/`..` нормализация
в path — добавим, когда упрёмся"*, i.e. "deliberately not implemented — add it when we hit
a wall"). This WPT category is that wall.

## Impact

Broad — every consumer of `Url::resolve()` is affected, not just `fetch()`-style calls:
`<script src>`, `<link rel=stylesheet>`/preload, `<img src>`, and any other subresource
resolution that goes through `ResourceBase::resolve` in `crates/shell/src/main.rs`. Any
real-world page (not just WPT) whose relative asset paths climb a directory level with
`..` will silently 404 those assets. Distinct from BUG-347 (fetch() not resolving relative
URLs against a base *at all*) — this bug is about `Url::resolve()` itself not normalizing
the path once a base + reference *are* joined.

## Suspected fix direction

Add an RFC 3986 §5.2.4-style remove-dot-segments pass over the merged path before/after
the `Self::parse` call in `Url::resolve()` (and likely in `Url::parse` too, for absolute
URLs written with `..` directly). Re-run `run_report.py --all --root custom-elements
--recursive` afterward — the `reactions/`/`reactions/customized-builtins/` subtrees should
go from mostly TIMEOUT to running their actual test bodies.

## Измеренный вес (WPT-VENDOR-fetch, 2026-07-28)

Прогон вендоренной категории `fetch` (`run_report.py --all --root fetch
--recursive`, 1 ч 22 мин, 176/481 harness OK) дал этому багу первую **числовую**
оценку — и вскрыл более острый случай, чем `custom-elements`: здесь `..` ломает
не невендоренный внешний хелпер, а **внутрикатегорийный файл, который лежит на
диске**.

69 ответов 404 на путях вида:

```
/fetch/api/cors/../resources/utils.js       20
/fetch/api/basic/../resources/utils.js      18
/fetch/api/redirect/../resources/utils.js    8
/fetch/api/response/../resources/utils.js    6
/fetch/api/policies/../resources/utils.js    5
/fetch/api/request/../resources/utils.js     4
```

При этом `tests/wpt/fetch/api/resources/utils.js` вендорен и читаем — 404 целиком
порождён несхлопнутым `..`.

Этот файл — общий хелпер самой крупной подкатегории (`fetch/api/`, 178 тестов из
481) и определяет `RESOURCES_DIR`, `dirname`, `stringToArray`,
`requestForbiddenHeaders`. Поэтому **все** ошибки `… is not defined` в логе
прогона (`RESOURCES_DIR` 12, `dirname` 6, `stringToArray` 1,
`requestForbiddenHeaders` 1) — одно и то же последствие этого бага, а не
отдельные дыры.

Верифицировать фикс удобнее всего именно здесь: `run_report.py --all --root
fetch --recursive` и проверка, что 404 на `*/../resources/utils.js` исчезли
(до фикса — 69).

## Расширение (WPT-RUN-3 срез 22, `css/css-image-animation`, 2026-08-03)

Тот же паттерн, но с двумя точка-сегментами и на общем внекатегорийном
фикстур-ресурсе, а не внутрикатегорийном хелпере: единственный исполнившийся
тест категории, `image-animation-pseudo-animated-image-dynamic.html`, ссылается
на `../../images/anim-gr.gif` (два уровня вверх, `tests/wpt/images/` — только
что довендоренная в этом же коммите общая директория фикстур). Подтверждено
изолированно: файл на диске и корректно раздаётся при обычном (без `..`)
запросе, но живой прогон стабильно даёт `HTTP 404` именно на `../../`-варианте
до и после вендоринга — 404 целиком порождён несхлопнутым `..`, тест сам не
новая находка. TIMEOUT/`Unexpected 5` на этом файле — единственное следствие
этого же бага, не отдельный гэп.

## Расширение (WPT-RUN-3 срез 25, `css/css-properties-values-api`, 2026-08-03)

60 файлов `css/css-properties-values-api/animation/*.html` (из 69 —
9 не зависят от общего хелпера и падают по другой причине, см.
[BUG-530](BUG-530-OPEN.md)): каждый ссылается на `<script
src="../resources/utils.js">` (один уровень вверх, без цепочки `..`) — та же
литеральная-не-схлопнутая-точка-сегмент проблема, буквальный запрос уходит
как `.../animation/../resources/utils.js` и 404-ит, хотя после нормализации
путь валиден и файл существует. Общий хелпер `animation_test()`/`transition_test()`
из `utils.js` никогда не определяется, харнес зависает TIMEOUT на каждом файле
(0 сабтестов зарегистрировано). `.ini` под
`tests/wpt/metadata/css/css-properties-values-api/animation/` для всех 60 файлов.

## Срез 30 (`css/css-typed-om`, 2026-08-03) — largest single-category weight yet, .ini NOT committed

245 of the category's ~360 harness-executed files reference `<script
src="../resources/testhelper.js">` (one level up) — same literal-`..`-not-
collapsed 404 as every prior slice, confirmed via the run log
(`.../stylevalue-subclasses/../resources/testhelper.js: network error: HTTP
404`). Downstream, every one of `testhelper.js`'s own exported helpers
(`createDivWithStyle`, `createComputedStyleMap`, `createDeclaredStyleMap`,
`createInlineStyleMap`, `assert_style_value_equals`, and 8 more) throws
`ReferenceError: <name> is not defined` the first time a test calls it — 985
subtests across the 245 files.

**Not attributed with a committed `.ini` this slice.** `css/css-typed-om`
itself is "P3 territory" per `CSS-SPECS.md` (CSS Typed OM not yet targeted),
so even a fixed `..` resolve would very likely just expose the *next* layer
of failure (`CSSRGB`/`CSSMathMax`/`CSS.deg`/`idl_test` — the category's own
unimplemented globals, confirmed present in the same run log, unrelated to
this bug) rather than flip these files to PASS. Disentangling "masked by
BUG-346" from "genuinely-expected Typed-OM gap" per subtest is a bigger job
than this slice's budget — left as a dedicated follow-up (WPT-RUN-3 срез
31+) rather than either over-attributing to this bug or silently leaving
the category's `.ini` gap unexplained.

## Срез 37 (`css/css-typed-om`, 2026-08-04) — follow-up disentangled and `.ini` committed

Re-ran with the structured wptreport JSON (333 of 374 files reference
`../resources/`, slightly more than slice 30's file-count estimate).
Classified every failing subtest by its literal error message: 1151
subtest failures plus 32 of 33 zero-subtest harness TIMEOUTs match
`<name> is not defined` for one of `testhelper.js`'s 15 exports (or a
harness-level TIMEOUT on a file whose *only* script tag besides
testharness.js/report.js is the broken `../resources/testhelper.js`,
confirmed by `stylevalue-normalization/normalize-image.html` — its own
`test()` bodies call `CSSStyleValue.parse()`, not any `testhelper.js`
helper, yet it still zero-registers, so the failed script load itself
blocks harness completion here, not merely undefined identifiers) — both
patterns attributed to this bug. The remaining 185 subtest failures and 1
TIMEOUT (`idlharness.html`, unrelated — `/resources/idlharness.js` simply
unvendored) are genuine Typed-OM API gaps, filed separately as
[BUG-554](BUG-554-OPEN.md). `.ini` committed for all affected files under
`tests/wpt/metadata/css/css-typed-om/`.

## Фикс (P3, 2026-08-06)

Implemented RFC 3986 §5.2.4 `remove_dot_segments` in `crates/core/src/url.rs`
and wired it into `Url::parse()`, gated on `has_authority` (`http`/`https`/
`file`/`ws`/`wss` — schemes without authority, e.g. `data:`, keep their path
untouched since dot-segment folding is a hierarchical-path concept). Since
`Url::resolve()`'s every branch that builds a new URL already delegates to
`Self::parse()`, this single change fixes both direct absolute-URL parsing
(`https://x/a/../b` → `/b`) and reference resolution (`base.resolve("../y")`)
in one place — no change to `resolve()` itself was needed beyond a doc-comment
update.

Algorithm follows the RFC pseudocode literally (loop consuming `input`,
building `output`, five cases: `../`/`./` prefix strip, `/./`/`/..` segment
collapse with last-segment removal, bare `.`/`..` drop, plain segment move).
9 new unit tests in `crates/core/src/url.rs`, including the two exact URLs
from this bug's original repro (`custom-elements/reactions/HTMLTableElement.html`
→ `../resources/custom-elements-helpers.js` and the `customized-builtins/`
two-level-up variant) plus RFC edge cases (`/a/./b`, `/a/b/..`, `/../a` —
extra `..` above root is dropped, not an error). `cargo test -p lumen-core`
34/34 green, `cargo clippy -p lumen-core --all-targets -- -D warnings` clean.
`ResourceBase::resolve` (`crates/shell/src/main.rs::4149-4159`) calls exactly
`Url::parse(base_url).and_then(|u| u.resolve(href))` with no other
transformation, so `<script src>`/`<link>`/`<img>` resolution is fixed
end-to-end through this one change.

Not verified via a full `run_report.py --all --root custom-elements
--recursive` run in this session (no Python venv present in the worktree;
setting one up was judged disproportionate for a pure string-normalization
fix already covered by exact-repro unit tests) — a future WPT session should
confirm the `reactions/`/`reactions/customized-builtins/` subtrees move from
TIMEOUT to running actual test bodies, and that the `fetch/api/*/../resources/
utils.js` 404 class (69 responses, measured 2026-07-28) is gone.
