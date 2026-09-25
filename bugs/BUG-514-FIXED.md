# BUG-514: `env()` CSS function not implemented in stylesheet parsing/cascade

**Статус:** FIXED 2026-09-24 (P3)
**Дата:** 2026-08-03
**Компонент:** css-parser (`grep -rn '"env"' crates/` finds no CSS-related
hit — only unrelated WebAssembly import-namespace strings in
`crates/js/src/wasm/tests.rs`/`webassembly.rs`)
**Найден:** WPT-RUN-3 срез 21 (`ROADMAP.md`) — массовый прогон `css/css-env`

## Механизм

`env()` (CSS Environment Variables Module Level 1,
`https://drafts.csswg.org/css-env-1/`) is the mechanism behind
`safe-area-inset-{top,right,bottom,left}` and similar UA-provided values —
shipped everywhere, actively used for notch/safe-area handling on mobile
web. It is not recognized as a function anywhere in `css-parser`: a
declaration whose value contains `env(...)`, at any nesting depth or in any
context (`@supports` condition, `var()` fallback, direct declaration
value), fails to resolve — the whole declaration is treated as invalid
rather than substituting the environment value, so the property falls back
to its own initial value (empty string surfaces through `getComputedStyle`,
since the declaration never took effect at all).

This is distinct from [BUG-484](BUG-484-FIXED.md) (inline `style` setter
accepts anything unvalidated): these failures come from **stylesheet**
declarations (`<style>` blocks, not `element.style = ...`), where the real
`css-parser` grammar is supposed to run — `env()` genuinely isn't a token
the grammar understands, it isn't merely unvalidated.

## Симптом

```
FAIL Test that CSS env vars work with @support
  assert_equals: expected "rgb(0, 128, 0)" but got ""
FAIL background-color: env(test) rgba(0, 0, 0, 0)
  assert_equals: expected "rgba(0, 0, 0, 0)" but got ""
FAIL Test unknown env() names will override previous values
  assert_equals: expected "rgba(0, 0, 0, 0)" but got ""
FAIL Test that CSS env vars work with CSS.supports
  assert_true: expected true got false
```

## Масштаб находки

5 files / 22 subtests: `syntax.tentative.html` (18 — the file's own
parametrized syntax-acceptance table, valid/invalid `env(...)` forms all
fail identically because none are ever substituted), `at-supports.tentative.html`
(1), `unknown-env-names-override-previous.tentative.html` (1),
`supports-script.tentative.html` (1, `CSS.supports("background",
"env(test)")` returns `false` for syntactically valid `env()` usages — same
root cause reached through the OM instead of the cascade),
`fallback-nested-var.tentative.html` (1, `env(test, var(--main-bg-color))`
fallback-to-`var()` never resolves).

Three more files in this category attribute elsewhere: `env-parsing.html` (5)
and `indexed-env.tentative.html` (4) fail on the generic inline-`style`
rejection gap ([BUG-484](BUG-484-FIXED.md) — malformed `env(...)` accepted by
`element.style` instead of rejected, a JS-layer issue independent of this
one), `env-revert-rule.html` (1) fails because `revert-rule` inside an
`env()` fallback is substituted textually before the cascade sees it, so the
cascade-level fix in [BUG-487](BUG-487-FIXED.md) does not reach it, and
`env-in-custom-properties.tentative.html` (2) dies earlier on bare
identifier access ([BUG-384](BUG-384-FIXED.md)).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-env/` for the 5
attributed files, `expected: FAIL` per subtest.

## Ревизия P3 2026-09-05 — премиса устарела, найдено 3 разных корня

`grep -rn '"env"' crates/` по-прежнему пуст (WebAssembly-контекст не CSS), но
это больше не значит «`env()` не распознаётся»: `env()`/`var()`-substitution
давно реализована в `crates/engine/layout/src/style/substitute.rs`
(`expand_env_vars`, разворачивание fallback, вложенный `calc()`/`var()`,
глубина рекурсии, 5 юнит-тестов `filter_transform_snap_mask.rs`), `@supports`
уже не проверяет значение деклараций вовсе (`SupportsCondition::evaluate`
всегда `true` для известного имени свойства — тот же wildcard, что для
`var()`), а `CSS-SPECS.md:114` числит модуль ✅. Живой прогон (`run_smoke.py`,
собранный `dev-release`) подтверждает: 1 из 5 файлов уже проходит
(`unknown-env-names-override-previous.tentative.html`, `.ini` удалён), а
остаток (4 файла/21 сабтест) распадается на **три независимых, не связанных
с "env() не реализован" причины**, установленные прямой живой пробой
(`--mcp-live-port`, `LUMEN_NO_ENGINE_THREAD=1`), а не по интуиции:

1. **`syntax.tentative.html` (18 сабтестов)** — каждый кейс создаёт `<div>`
   через `document.createElement`, ставит `elem.style.cssText`, читает
   `getComputedStyle(elem)` в том же тике скрипта. Проба показала: даже
   БАЗОВЫЙ кейс без единого `env()` (`div.style.cssText = ''`, ожидание —
   цвет из `<style>`) и вообще любое свойство (`display`/`color` на только
   что созданном узле) отдают `""` — классический
   [BUG-493](BUG-493-FIXED.md)/CSSOM-4 (`getComputedStyle` не форсирует
   flush, недавно созданный узел не виден). Реатрибутировано на BUG-493.
2. **`at-supports.tentative.html` + `fallback-nested-var.tentative.html`
   (по 1 сабтесту)** — оба читают
   `getComputedStyle(document.body).backgroundColor`. Прямая проба нашла
   ТРЕТИЙ, ранее не заведённый баг, не имеющий отношения к `env()`:
   canvas-background-propagation (`box_tree/entry.rs::propagate_canvas_background`)
   безусловно вычищает `background-color`/`background-layers` из
   `body`'s собственного `Arc<ComputedStyle>` (не копия — тот же объект,
   что видит CSSOM), поэтому `getComputedStyle(body).backgroundColor`
   отвечает `transparent` для ЛЮБОГО `body { background-color: ... }`,
   вне зависимости от `env()`/`@supports`. Заведено отдельно —
   [BUG-1103](BUG-1103-FIXED.md), исправлен P6 2026-09-23: мутация
   `ComputedStyle` убрана целиком, `canvas_background_color()` читает
   пропагируемый цвет read-only, `getComputedStyle(body)` больше не
   искажается.
3. **`supports-script.tentative.html` (1 сабтест)** — не CSSOM-маскировка:
   реальный, узкий, ФИКСНУТЫЙ здесь дефект. `CSS.supports("background",
   "env()")` (пустые скобки, без обязательного `<custom-ident>`) отвечал
   `true` — двухаргументная форма `_lumen_css_supports_prop`
   (`crates/js/src/v8_runtime/install/platform.rs`) намеренно игнорирует
   `value` целиком (Phase-0 упрощение для feature-detection). Добавлена
   узкая проверка `value_has_empty_env_call` — только детектирует пустые
   `env()` (нет общей ревалидации значения, не расширяет объём Phase-0
   упрощения), 2 юнит-теста. Живой прогон подтвердил:
   `supports-script.tentative.html` теперь проходит, `.ini` удалён.

Точечного P3-фикса для `env()` самого по себе не требовалось — он уже
реализован. Остаток (19 сабтестов) не блокируется этой заявкой: 18 —
BUG-493/CSSOM-4 (P1-трек), 2 — BUG-1103 (отдельная заявка, нужен
`LayoutBox`-level маркер, вне точечного фикса).

## Исправление P3 2026-09-24 — два настоящих дефекта самого `env()`

Перемер 2026-09-24 (свежий `dev-release`, `run_smoke.py` по всем 10 файлам
`css/css-env`) показал, что вывод ревизии 2026-09-05 «`env()` сам по себе
реализован, остаток чужой» неверен. `syntax.tentative.html` больше не
маскируется BUG-493 (базовый кейс без `env()` уже проходил), и 14 его
сабтестов падали **на самом `env()`**, а
`unknown-env-names-override-previous.tentative.html`, который 09-05 проходил,
снова падал (`rgb(0, 128, 0)` вместо `rgba(0, 0, 0, 0)`). Корней два.

1. **Каскад: invalid at computed-value time сливался с ошибкой разбора.**
   `apply_declaration` (`crates/engine/layout/src/style/apply.rs`) на
   нераскрываемой подстановке делал `return`, то есть «декларации не было», и
   свойство оставалось с предыдущей декларацией (`background-color: green;
   background-color: env(unknown)` → зелёный). По CSS Variables L1 §3.3 /
   CSS Env L1 §3 такое свойство вычисляется как `unset`. При этом кривой
   `env()` (`env(10px)`, `env(env(test))`, `env(test, {)`) — наоборот,
   ошибка разбора, и должен давать именно «декларации не было»; раньше он
   давал то же самое лишь случайно. Теперь:
   * `substitute.rs::env_calls_well_formed` проверяет грамматику
     `env( <custom-ident> <integer [0,∞]>* , <declaration-value>? )`
     (плюс баланс скобок всего значения и комментарии как пробелы); кривой
     `env()` — декларация выбрасывается;
   * правильная декларация с `var()`/`env()` сначала сбрасывает свойство в
     `unset`, и если подстановка не удалась (или свойство не приняло
     результат), сброс и остаётся итогом;
   * `find_env_open` стал регистронезависимым (`ENV(test)`) и не матчит
     `env(` внутри чужого идентификатора.
   `apply_css_wide_keyword` теперь зовётся на каждую такую декларацию, поэтому
   `ComputedStyle::root()` для `initial` кэшируется в `thread_local` вместо
   сборки на каждый вызов.
2. **Inline-`style` CSSOM отбрасывал любое значение с `env()`/`var()`.**
   `_lumen_canonicalize_longhand` / `setProperty`
   (`crates/js/src/shim/web_api_shim_mid.js`) прогоняли такие значения через
   грамматику свойства (`<color>`, `<length>`), которая `env(...)` не знает —
   `el.style.width = 'env(safe-area-inset-top)'` и `cssText =
   'background-color: env(test)'` молча исчезали. Значение с подстановкой
   проверяется только после подстановки, поэтому теперь оно обходит
   канонизаторы (и раскрытие шортхендов) и хранится как есть; на этапе
   разбора проверяется только грамматика `env()` тем же валидатором, что и
   в каскаде (натив `_lumen_css_env_well_formed`).

**Замер до/после** (`run_smoke.py`, те же 10 файлов, одна машина): PASS
сабтестов 18/42 → **41/42**. `syntax` 4/18 → 18/18, `env-parsing` 5/8 → 8/8,
`indexed-env` 4/8 → 8/8, `seralization-round-tripping` 0/1 → 1/1,
`unknown-env-names-override-previous` 0/1 → 1/1. Остался только
`env-revert-rule.html` — `revert-rule` в fallback `env()`, отдельный дефект
(подстановка текстом до каскада). `.ini` проходящих файлов удалены.

**Та же граница parse-error / IACVT для `var()`.** Поскольку сброс в `unset`
стал общим для любой подстановки, значения, которые раньше «пропускались»
случайно, пришлось явно отнести к ошибкам разбора
(`substitute.rs::substitution_value_well_formed` + голова `var()` в
`env_calls_well_formed`): bad-string/bad-url (перевод строки в строке),
несогласованная закрывающая скобка, `!` верхнего уровня (второй `!important`),
`var()` с головой не из одного `--ident` (`var(--x ())`). `revert-layer` /
`revert-rule`, пришедшие из fallback, по-прежнему не сбрасывают свойство —
каскад разрешает их до apply только по литеральному значению.

**`css/css-variables` (241 файл, A/B на одном `main` 2c76d21be): PASS
сабтестов 379 → 413, регрессий ноль.** Выросли `variable-reference.html`
8 → 16/18, `var-parsing` 5 → 8/8, `css-variable-change-style-001` 5 → 9/9,
`variable-substitution-shorthands` 39 → 43/51,
`variable-presentation-attribute` 21 → 24/48, `variable-reference-cssom`
0 → 2/2, `variable-reference-shorthands-cssom`, `variable-cssText`,
`variable-substitution-basic`, reftest-ы `variable-declaration-24/25/29/48/50`,
`variable-reference-02/19`. `.ini` для `var-parsing` и
`css-variable-change-style-001` удалены; остальные `.ini` каталога уже до
этой правки содержали устаревшие `UNEXPECTED-PASS` — не трогались.

Юнит-тесты — 7 новых в
`crates/engine/layout/src/tests/filter_transform_snap_mask.rs`.
