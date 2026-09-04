# BUG-508: CSS Color HDR `dynamic-range-limit` property entirely unimplemented

**Статус:** OPEN (ДОРАБОТКА → CSS-SPECS.md)
**Тип:** доработка — остаток (интерполяция/Web Animations не проведены живьём)
требует расширения Phase-0 animatable-таблицы движка (`crates/engine/layout/
src/animation.rs`), разделяемой всеми анимируемыми свойствами, не точечная
правка одного свойства; см. ревизию P3 2026-09-04 ниже
**Дата:** 2026-08-02
**Компонент:** css-parser/layout — property not recognized anywhere
(`grep -rn "dynamic-range-limit\|dynamic_range_limit" crates/` returns zero hits)
**Найден:** WPT-RUN-3 срез 14 (`ROADMAP.md`) — массовый прогон `css/css-color-hdr`

## Симптом

`dynamic-range-limit` (CSS Color HDR Module Level 1) is not parsed, not
placed in `ComputedStyle`, and does not participate in interpolation/Web
Animations at all — every test that probes the property fails the same way:

- `computed.html` (21/21 FAIL): every candidate keyword/`dynamic-range-limit-mix()`
  value fails `assert_true: dynamic-range-limit doesn't seem to be supported
  in the computed style expected true got false`.
- `inheritance.html` (2/2 FAIL): same assertion for "has initial value" and
  "inherits".
- `interpolation.html` (64/64 FAIL, all four interpolation methods — CSS
  Animations, CSS Transitions ×2 forms, Web Animations — for the same
  underlying reason): `assert_true: 'from' value should be supported
  expected true got false` / `assert_true: Web Animations should be
  supported expected true got false`.

`parsing.html`'s 16 failures (13/29 subtests pass) are a *different*,
already-tracked mechanism entirely — the inline `style` setter never
rejects invalid values ([BUG-484](BUG-484-OPEN.md)) — and are not
attributed to this bug.

## Масштаб находки

3 files / 87 subtests directly attributable to the missing property
(`computed.html` 21, `inheritance.html` 2, `interpolation.html` 64).

## .ini

Committed `.ini` under `tests/wpt/metadata/css/css-color-hdr/` for
`computed.html`, `inheritance.html`, `interpolation.html` (per-subtest
`expected: FAIL`).

## Ревизия P3 2026-09-04: точечный дефект устранён, интерполяция переклассифицирована в ДОРАБОТКА

**Реализовано и подтверждено.** Новый тип `DynamicRangeLimit`
(`crates/engine/layout/src/style/values/dynamic_range_limit.rs`): keyword
(`standard`/`constrained`/`no-limit`) + `dynamic-range-limit-mix()` с
произвольной вложенностью, флаттенинг в нормализованный 3-компонентный вес
(канонический порядок `standard, constrained, no-limit`, ровно правила
ренормализации/коллапса `computed.html`) — проведено через
`ComputedStyle::dynamic_range_limit` (наследуемое, initial `no-limit`),
`apply_decl_paint`, `apply_css_wide_keyword`, `compute_style`'s inherit
branch, `computed_style_to_map` (`getComputedStyle`) и `SUPPORTED_PROPERTIES`
(`CSS.supports`). 14 юнит-тестов (`style/tests/dynamic_range_limit_tests.rs`)
зеркалят каждую проверку `parsing.html`/`computed.html`/`inheritance.html`/
`interpolation.html`, все зелёные. Заголовочная формулировка бага
(«property entirely unimplemented») больше не верна — свойство парсится,
хранится, наследуется и сериализуется корректно.

`computed.html`/`inheritance.html` живьём (`run_report.py --all --root
css/css-color-hdr --recursive --update-expected` на свежей `dev-release`)
по-прежнему в основном красные — но НЕ из-за этого бага: каждый упавший
сабтест мутирует и читает `getComputedStyle()` в ОДНОМ синхронном тике
скрипта, а [BUG-493](BUG-493-OPEN.md) (`getComputedStyle()` не форсирует
синхронный релейаут перед чтением кэша) отдаёт снимок до мутации — тот же
механизм, что уже задокументирован в [BUG-495](BUG-495-OPEN.md) для
`background-position-{x,y}`. `.ini` обоих файлов обновлены: тот же список
FAIL, что и раньше, но заголовок теперь атрибутирует остаток к BUG-493, не
к «свойство не распознаётся». `parsing.html` не затронут, остаётся
[BUG-484](BUG-484-OPEN.md) (комментарий `.ini` восстановлен после того, как
`--update-expected` его стёр — содержимое не изменилось).

**Интерполяция/Web Animations — НЕ реализована живьём, остаётся ДОРАБОТКОЙ.**
`DynamicRangeLimit::interpolate` (компонентный lerp, CSS Color HDR L1 §2.1)
существует и отзеркален в JS-шиме Web Animations
(`_wa_lerp_dynamic_range_limit`, `crates/js/src/shim/web_api_shim_tail_b.js`,
проверено построчным сравнением с ожиданиями `interpolation.html` через
`node -e`), но **ничем не вызывается вживую**: ни нативный движок CSS
Animations/Transitions (`crates/engine/layout/src/animation.rs`'s
`TransitionScheduler`/`AnimationScheduler` — захардкоженная таблица ровно из
5 анимируемых свойств, `opacity`/`color`/`background-color`/`transform`/
`height`; тот же пред-существующий Phase-0 предел, что уже
задокументирован необязательным для этой находки — ROADMAP.md CC-11
называет его «пред-существующим ограничением, не пробелом среза»), ни путь
`element.animate()` (`Animation.prototype._applyAtP` пишет в inline-стиль,
который становится JS-видимым только после релейаута шелла).

Живой WPT формально даёт `interpolation.html` **64/64 pass**, но это **ложный
проход**, не подтверждение работы: `interpolation-testcommon.js` создаёт
actual/expected элементы, интерполирует и меряет — всё в одном синхронном
тике, без релейаута шелла между вставкой узла и чтением
`getComputedStyle()`; оба узла читаются как `''`, сравнение вырождается в
`'' === ''`. Ровно тот же паттерн уже наблюдается в вендоренном дереве для
заведомо неанимируемых свойств — `tests/wpt/metadata/css/css-break/
animation/{orphans,widows}-interpolation.html.ini` перечисляют только
Web Animations сабтесты как FAIL, а CSS Animations/Transitions — как
формально «passing» тем же вырожденным механизмом. `.ini` для
`interpolation.html` оставлен пустым (удалён), отражая фактический вывод
wptrunner — но **не следует читать это как «интерполяция работает»**; когда
BUG-493 будет исправлен, этот файл должен покраснеть по-настоящему, и
только тогда останется реальная работа: добавить `dynamic-range-limit` в
`animatable`-таблицу `animation.rs` (нужен новый вариант `AnimValue`,
несущий `DynamicRangeLimit`, плюс поддержка в `parse_keyframe_style`) и
кейс `dynamicRangeLimit` в JS `_wa_interp_prop` — обе точки уже
идентифицированы, код не написан.

**Переклассификация:** статус переведён в `OPEN (ДОРАБОТКА → CSS-SPECS.md)`
— расширение Phase-0 animatable-таблицы движка касается не только этого
свойства и не точечная P3-правка (`animation.rs` уже за пределом в 2000
строк, нужен предварительный сплит перед любым расширением). Строка снята
с `STATUS-P3.md`; `CSS-SPECS.md`'s T4-таблица (`dynamic-range-limit`, 🟡)
несёт актуальный статус.

Гейты: `cargo test -p lumen-layout --lib dynamic_range_limit` 14/14,
`cargo clippy -p lumen-css-parser --all-targets -- -D warnings` чисто,
`cargo check -p lumen-layout --all-targets` чисто (полный `--workspace`
clippy блокирован предсуществующим несвязанным дрейфом линтера в
`lumen-image` — версия системного `rustc` новее пина, см.
`feedback_linux_toolchain_mismatch` в памяти сессии).
