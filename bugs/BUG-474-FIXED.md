# BUG-474: `document.caretRangeFromPoint` not implemented

**Статус:** FIXED 2026-09-02 (P3)
**Дата:** 2026-08-02
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js`)
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

## Fixed 2026-09-02 (P3)

Добавлен `document.caretRangeFromPoint(x, y)` в
`crates/js/src/shim/web_api_shim_mid.js`, рядом с `caretPositionFromPoint`
и его собственной Phase-0 заглушкой (`dom.rs:7396-7402`'s заглушка тем
временем переехала в шим вместе со SPLIT-JS3, см. её актуальную позицию
там же). Тот же Phase-0 подход: при наличии `body` — коллапсированный
`Range` на `body` offset 0 через уже существующий `_lumen_make_range`;
без `body` — `null`. Реального layout hit-testing по координатам x/y как
не было для `caretPositionFromPoint`, так и не появилось здесь.

Побочная находка при проверке заявленного сабтеста
(`range instanceof Range`): `_lumen_make_range` строил ПРОСТОЙ объект без
привязки к `Range.prototype`, так что `instanceof Range` был `false` для
ЛЮБОГО Range в движке (`document.createRange()`, `Selection`-диапазоны,
`new Range()`) — без починки этого даже реализованный метод не проходил
первый же сабтест заявки. Добавлен
`Object.setPrototypeOf(r, Range.prototype)` в конце `_lumen_make_range`.

Живой прогон `run_report.py` (3 файла семейства
`css/cssom/caretRangeFromPoint*`) против свежей сборки:

- `caretRangeFromPoint.tentative.html`: 2/8 сабтестов теперь PASS —
  «no supplied coordinates» (сабтест из заявки) и «on a shadow … same
  node as caretPositionFromPoint» (оба стаба сходятся на одном и том же
  `body`,0). Остальные 6 требуют реального hit-testing (границы viewport,
  посимвольное разрешение, canvas/input) — не тронуто этим фиксом,
  остаётся тем же нереализованным Phase-0 куском, что и у
  `caretPositionFromPoint`.
- `caretRangeFromPoint-textarea-transform.tentative.html`: без изменений,
  0/2 — тоже нужен реальный hit-testing внутри трансформированного
  `<textarea>`.
- `caretRangeFromPoint-replace-document.tentative.html`: падает раньше на
  несвязанный `document.replaceChild is not a function` — отдельный,
  ранее не заводившийся дефект (см. «Масштаб находки» выше), вне скоупа
  этого фикса. `.ini` для него сгенерирован (`--update-expected`) как
  baseline, но не заводился как отдельный BUG-NNN.

`.ini` всех трёх файлов регенерированы `run_report.py --update-expected`
под фактический результат этого прогона.

Регресс-тесты: `caret_range_from_point_exists`,
`caret_range_from_point_returns_range_instance_with_zero_offsets`
(`crates/js/src/dom/tests/v8_window_anim_compress.rs`).

Гейты: `cargo clippy -p lumen-js --all-targets --features v8-backend --
-D warnings` чисто; `cargo test -p lumen-js --features v8-backend`
3392/3392 (весь крейт, включая оба новых теста).
