# BUG-470: `getComputedStyle()` does not expose `float`/`clear`

**Статус:** OPEN
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
