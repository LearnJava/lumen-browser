# BUG-318: MutationObserver — некорректный учёт записей и недоставка subtree-мутаций

**Renumbered 2026-07-18** from `BUG-316` — collided with `origin/main`'s own
`BUG-316` (`cpu_snapshots_match_references` gap, itself already renumbered
twice by other parallel sessions), resolved while merging S6/S7 back into
`main`.

**Статус:** OPEN
**Дата:** 2026-07-18
**Компонент:** js (WEB_API_SHIM, `crates/js/src/dom.rs`)
**Найден:** P2-wpt S6, курируемый асинхронный DOM-сабсет через `wptrunner`

## Симптом

Учёт мутационных записей в шиме `MutationObserver` неполон/неверен под V8:

- **Дублирование/лишние записи + `takeRecords()` не очищает очередь.**
  `dom/nodes/MutationObserver-takeRecords.html` (harness `OK`):
  - `unreachabled test` → колбэк сработал, когда не должен был (`assert_unreached`).
  - `All records present` → `mutation records must match expected 5 but got 6`.
  - `No more records present` → `expected 0 but got 4` (записи доставлены после
    `takeRecords()`, который должен был их изъять).
- **Subtree-мутации не наблюдаются.** `dom/nodes/MutationObserver-disconnect.html`
  (harness `TIMEOUT`): при `observe(..., {subtree:true})` мутации в потомках не
  приводят к вызову колбэка → оба async-сабтеста уходят в timeout
  (`subtree mutations`, `disconnect discarded some mutations`).

Все провалы записаны как `expected: FAIL`/`expected: TIMEOUT` — тесты не ослаблены.

## Ожидание

DOM Standard §4.3: колбэк получает ровно те записи, что соответствуют
зарегистрированным опциям; `takeRecords()` возвращает и **очищает** очередь;
`subtree:true` наблюдает мутации во всём поддереве. Сейчас обёртки-перехватчики
(`_lumen_append_child`/`_lumen_remove_child`/…) уведомляют только прямой target,
а не сопоставляют мутацию с зарегистрированными наблюдателями предков.

## Воспроизведение

```bash
LUMEN_PROFILE=dev-release tests/wpt/.venv/Scripts/python.exe \
  tests/wpt/run_smoke.py /dom/nodes/MutationObserver-takeRecords.html \
                         /dom/nodes/MutationObserver-disconnect.html
```
