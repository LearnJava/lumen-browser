# BUG-470: `getComputedStyle()` does not expose `float`/`clear`

**Статус:** FIXED 2026-09-01
**Дата:** 2026-08-02
**Компонент:** js/layout (computed-style reflection object)
**Найден:** WPT-RUN-3 срез 2 (`ROADMAP.md`) — массовый прогон `css/CSS2`,
`css/CSS2/floats/inheritance.html` (4/4 сабтеста FAIL)

## Симптом

```
FAIL Property float has initial value none - assert_true: float doesn't seem to be supported in the computed style expected true got false
FAIL Property float does not inherit - assert_true: expected true got false
FAIL Property clear has initial value none - assert_true: clear doesn't seem to be supported in the computed style expected true got false
FAIL Property clear does not inherit - assert_true: expected true got false
```

`css/support/inheritance-testcommon.js`'s `assert_not_inherited()` начинает с
`assert_true(property in getComputedStyle(target))` — для `float`/`clear` это
`false`: свойства нет среди ключей объекта, который возвращает
`getComputedStyle()`, хотя float-раскладка сама по себе работает (Lumen
позиционирует float'ы, см. соседние find'ы того же среза, BUG-464/BUG-469, где
сами float'ы влияют на геометрию). Это гэп именно в отражении вычисленного
стиля в JS-объект, а не в layout-движке.

## .ini

`tests/wpt/metadata/css/CSS2/floats/inheritance.html.ini` — 4 сабтеста
`expected: FAIL`.

## Корень

`getComputedStyle()` (`crates/js/src/shim/web_api_shim_tail_b.js`) возвращает
`Proxy({}, handler)` — цель прокси пустой объект-литерал. Ловушки `get`/
`getPropertyValue` резолвят свойство через `_lumen_computed_property`, но
ловушки `has` не было вовсе, поэтому оператор `in` падал на `Reflect.has`
целевого пустого объекта и отвечал `false` для ЛЮБОГО свойства, не только
`float`/`clear` — сам список ~64 свойств в `computed_style_to_map`
(`selector_query.rs`) уже включал оба задолго до этого бага (соседние
BUG-464/BUG-469 подтверждают, что сама float-раскладка работает). Все четыре
провала `inheritance-testcommon.js` начинаются именно с `assert_true(property
in getComputedStyle(target))` — единственная точка отказа.

## Фикс

Добавлена ловушка `has` в тот же Proxy-хендлер, зеркалящая эвристику `get`
(непустая строка `_lumen_computed_property` → `true`), плюс всегда-`true` для
служебных методов (`getPropertyValue`/`length`/`item`/`cssText`). Тест
`computed_style_in_operator_reports_known_properties`
(`crates/shell/src/tests/page_pipeline.rs`) проверяет `'float' in cs`, `'clear'
in cs`, `'__lumenBogusProp__' in cs` (должно быть `false`) и
`cs.getPropertyValue('float')`.

Гейты: `cargo test -p lumen-shell` — новый тест `ok`; `cargo clippy --workspace
--all-targets -- -D warnings` чисто; `bash scripts/scoped-test.sh` (замыкание
lumen-js/lumen-shell) — всё `ok`, включая `h3::udp::tests::udp_round_trip`
(BUG-805 в этом прогоне не воспроизвёлся).
