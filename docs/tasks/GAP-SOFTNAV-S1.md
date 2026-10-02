# GAP-SOFTNAV-S1 — синхронная атрибуция мягкой навигации

Срез 1 из GAP-SOFTNAV (BUG-678). Владелец — P1. Крейт — `lumen-js`.

## Цель

Минимальная детекция Soft Navigations (WICG) **без** атрибуции через промисы и таймеры (S2) и без LCP/ICP (S3):

1. Пока идёт dispatch доверенного (`isTrusted`) `click` или `keydown`, держать флаг «interaction context» с меткой времени начала. Флаг снимается по выходу из dispatch.
2. Внутри контекста фиксировать:
   - `history.pushState` / `replaceState` со сменой URL;
   - вставку узла в присоединённый документ (`appendChild` / `insertBefore` / `append` и т. п.).
3. Если в одном контексте случилось и то и другое: на ближайшем rAF, если вставленный узел присоединён и у него ненулевой бокс, вызвать `_lumen_deliver_soft_nav(url, startTime, 0)`. Не больше одной записи на взаимодействие.
4. Добавить `'soft-navigation'` в `_PERF_SUPPORTED_ENTRY_TYPES`.

## Точки входа

- [soft_navigation.rs:66](../../crates/js/src/soft_navigation.rs#L66) `_lumen_deliver_soft_nav`.
- `pushState` / `replaceState` — [web_api_shim_mid_b.js:1190](../../crates/js/src/shim/web_api_shim_mid_b.js#L1190), `:1206`.
- Dispatch событий — `web_api_shim_mid.js` около `:1331-1340`.
- [web_api_shim_tail.js:30](../../crates/js/src/shim/web_api_shim_tail.js#L30) `_PERF_SUPPORTED_ENTRY_TYPES`.

## Не трогать

- Shell (LCP, `PersistentJs::deliver_lcp_entry`).
- `navigationId` на других записях (S4).
- Распространение контекста через microtask и таймеры (S2 — проектный вопрос).

## Готово, когда

1. `supported-entry-types.window.js`: FAIL → PASS.
2. `smoke/tentative/basic.html`: ERROR → OK, оба сабтеста PASS.
   - Сначала пробой выяснить причину нынешнего ERROR.
   - Если она вне JS-шима (например, клик из BiDi приходит без `isTrusted`), записать её в ROADMAP-строку. Тогда критерий — только п. 1 и п. 3.
3. Юнит-тест в `soft_navigation.rs`: синтетический trusted click → pushState + appendChild → после rAF ровно одна запись `soft-navigation` с нужным `name`. Без клика записей нет.
4. Прогон `run_report.py --all --root soft-navigation-heuristics --check` даёт exit 0, baseline обновлён (`--update-expected`).

## Гейт

```
cargo clippy -p lumen-js --features v8-backend --all-targets -- -D warnings
bash scripts/scoped-test.sh
python graphic_tests/dump_golden.py
```

## Зависимости

Нет.
