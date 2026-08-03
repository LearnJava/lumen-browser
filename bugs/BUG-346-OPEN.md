# BUG-346: `Url::resolve()` doesn't collapse `.`/`..` dot-segments — relative subresource URLs with `..` 404

**Статус:** OPEN
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
