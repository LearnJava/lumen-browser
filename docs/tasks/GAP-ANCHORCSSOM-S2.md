# GAP-ANCHORCSSOM-S2 — `element.style` для `anchor()` / `anchor-size()` через настоящий парсер

Срез 2 из GAP-ANCHORCSSOM (BUG-563). Владелец — P1. Крейты — `lumen-layout` и `lumen-js` (соседние).

## Цель

Значение инсетов (`top`/`left`/`right`/`bottom`/`inset-*`) и размеров (`width`/`height`/`min-*`/`max-*`) — это `anchor(...)` или `anchor-size(...)` **на верхнем уровне**. Сейчас при присваивании через `element.style` оно не сохраняется как сырой текст и не отбрасывается целиком. Нужно:

1. разобрать его парсером из S1;
2. сериализовать канонически: порядок `<anchor-name> <anchor-side>`, fallback через `, `;
3. невалидное значение отвергать.

## Точки входа

- [web_api_shim_mid.js:2958](../../crates/js/src/shim/web_api_shim_mid.js#L2958) `_lumen_canonicalize_longhand`.
- `web_api_shim_mid.js:2293` `_LUMEN_LENGTH_PROPERTIES`.
- Регистрация нативов `_lumen_css_canonical_*` — [platform.rs:546-590](../../crates/js/src/v8_runtime/install/platform.rs#L546).
- Образец канонизатора — [length.rs:424](../../crates/engine/layout/src/style/values/length.rs#L424) `canonical_specified_*`.

Добавить `canonical_specified_anchor` (layout) и обращение к нему из length- и sizing-ветки, если значение начинается с `anchor(` или `anchor-size(`.

Сначала пробой проверить текущее поведение `el.style.top = "anchor(--a top)"`: ROADMAP:933 описывает сырой passthrough, но `top` уже входит в `_LUMEN_LENGTH_PROPERTIES`.

## Не трогать

- `anchor()` внутри `calc()` / `min()` / `max()`.
- Вложенный fallback-`anchor()`.
- `position-area`.

Это S3/S4, им нужно проектное решение.

## Готово, когда

1. В WPT `css/css-anchor-position/` уменьшилось число FAIL. Базовая линия: `anchor-parse-valid` 788, `anchor-parse-invalid` 25, `anchor-size-parse-valid` 1436, `anchor-size-parse-invalid` 22 (`.ini` в `tests/wpt/metadata/css/css-anchor-position/`).
2. `run_report.py --all --root css/css-anchor-position --check` даёт exit 0.
3. Baseline обновлён через `--update-expected`, числа «до/после» записаны в коммит.

Остаток FAIL — формы с calc() (S3). Перечислить их в ROADMAP-строке.

## Гейт

```
cargo clippy -p lumen-layout --all-targets -- -D warnings
cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

## Зависимости

GAP-ANCHORCSSOM-S1.
