# BUG-474: `document.caretRangeFromPoint` not implemented

**Статус:** OPEN
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/dom.rs`)
**Найден:** WPT-RUN-3 срез 3 (`ROADMAP.md`) — массовый прогон `css/cssom`

## Симптом

```
FAIL document.caretRangeFromPoint() (no supplied coordinates) returns Range with 0 0 values
  - document.caretRangeFromPoint is not a function
```

`grep -n "caretRangeFromPoint" crates/js/src/dom.rs` — ноль совпадений;
метод отсутствует вовсе. Не путать с `document.caretPositionFromPoint`
(CSSOM View §5.1), который реализован, но как Phase-0 заглушка
(`dom.rs:7396-7402`, комментарий "no layout hit-testing yet; returns body at
offset 0") — это отдельный, ранее не заводившийся дефект (эта заглушка не
является причиной ни одного FAIL/TIMEOUT в текущем срезе — все встреченные
`caretPositionFromPoint`-провалы объясняются уже открытыми
[BUG-384](BUG-384-FIXED.md) и [BUG-462](BUG-462-OPEN.md), см. `.ini`).

## Масштаб находки

2 файла: `caretRangeFromPoint.tentative.html`,
`caretRangeFromPoint-textarea-transform.tentative.html`. Третий файл того же
семейства, `caretRangeFromPoint-replace-document.tentative.html`, падает
раньше на `document.replaceChild is not a function` (Node-метод отсутствует
на живом `document`) — отдельное, не кластеризуемое по одному наблюдению;
без `.ini`, см. `docs/wpt-status.md` → строка `css`.

## .ini

Committed `.ini` под `tests/wpt/metadata/css/cssom/` для 2 атрибутированных
файлов, `expected: FAIL` по фактическому результату прогона.
