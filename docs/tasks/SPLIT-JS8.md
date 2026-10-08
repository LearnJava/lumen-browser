# SPLIT-JS8 — lumen-js: файлы сверх потолка 2000 строк

Дорожка SPLIT (BUG-1100). Владелец — P1. Крейт — `lumen-js`. Метод — [p1-monolith-split-queue.md](p1-monolith-split-queue.md) §2, §3, §5.

## Файлы (длины на 2026-10-02)

| Файл | Строк | Как резать |
|---|---|---|
| `crates/js/src/svg.rs` | 2742 | (1) `mod tests_v8` (2209–2743) → `src/svg/tests_v8.rs`. (2) Первый raw-кусок `concat!` в `SVG_SHIM` (строки 16–2197, `r#"…"#`) → `src/shim/svg_shim_head.js` через `include_str!`, как уже сделано для `svg_idl_table.js`/`svg_idl_shape.js` и SPLIT-JS3. Raw-строка переносится байт в байт, экранирования нет. |
| `crates/js/src/v8_runtime/install/dom_core.rs` | 2069 | `fn install_selection` (1621–1795) и `fn install_design_mode` (1818–2070) → новый `install/dom_editing.rs`; вызовы — через путь модуля. |
| `crates/js/src/dom/tests/v8_perf_observers.rs` | 2352 | с `// MutationObserver tests` (1030) до конца (MutationObserver, ResizeObserver, IntersectionObserver) → `dom/tests/v8_dom_observers.rs` |
| `crates/js/src/dom/tests/v8_ws_sse.rs` | 2162 (baseline) | с `// WebSocket API` (931) до конца → `dom/tests/v8_websocket_sse.rs` |
| `crates/js/src/canvas2d.rs` | 2002 (baseline) | опционально: вынести любой самодостаточный хелпер, чтобы стало ≤2000 |

Тестовые модули `dom/tests/` регистрируются в `crates/js/src/dom/tests/mod.rs:175-181`.

## Правило

Чисто механический перенос, без правок логики. JS-текст переносится байт в байт: V8 по-прежнему компилирует ту же строку, порядок кусков `concat!` сохраняется. Число тестов `cargo test -p lumen-js --features v8-backend` до и после совпадает.

## Не трогать

- `web_api_shim_*.js`.
- `worker.rs`, `frame_bridge.rs`, `filesystem_access.rs`, `web_audio.rs`, `offscreen_canvas.rs` — выросли сверх baseline, но это отдельные батчи.

## Готово, когда

- Перечисленные файлы ≤2000 строк, новые тоже.
- `python scripts/check_file_sizes.py 2>&1 | grep -E "js/src/(svg|canvas2d|dom/tests/v8_(perf_observers|ws_sse)|v8_runtime/install/dom_core)"` пусто.
- Строки baseline для `v8_ws_sse.rs` и `canvas2d.rs` удалены вручную, если файлы ушли под 2000. **Без** `--update`.
- §4 плана дополнен.

## Гейт

```
cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

## Зависимости

Нет.
