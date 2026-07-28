# BUG-318: MutationObserver — некорректный учёт записей и недоставка subtree-мутаций

**Renumbered 2026-07-18** from `BUG-316` — collided with `origin/main`'s own
`BUG-316` (`cpu_snapshots_match_references` gap, itself already renumbered
twice by other parallel sessions), resolved while merging S6/S7 back into
`main`.

**Статус:** FIXED 2026-07-20 (P3)
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

## Причина (4 дефекта в `_mo_*` шима)

1. **`observe()` не перерегистрировал наблюдателя.** Только конструктор
   `MutationObserver` делал `_mo_observers.push(this)`; `disconnect()` делал
   `splice` из `_mo_observers`, а последующий `observe()` добавлял лишь запись
   в `this._observations`, не возвращая наблюдателя в активный список. Поэтому
   после `disconnect()` + `observe()` (паттерн `MutationObserver-disconnect.html`)
   `_mo_notify` никогда не находил наблюдателя → колбэк не срабатывал (TIMEOUT).
2. **Нет проверки поддерева для `subtree:true`.** `_mo_notify` матчил запись,
   если `opts.subtree` истинно, **без** проверки, что мутировавший узел реально
   потомок `entry.target`. Любая мутация где угодно в документе (включая правки
   собственной results-таблицы `testharness.js`) сыпалась в очередь → лишние
   записи и `unreachabled test`, срабатывающий на постороннем изменении.
3. **`element.textContent` эмитил `characterData` вместо `childList`.** По DOM
   §4.9.1 присваивание `textContent` элементу заменяет всех детей одним текст-узлом —
   это `childList`-мутация (removedNodes = старые дети, addedNodes = новый узел).
4. **У живого текст-узла отсутствовал сеттер `.data`/`.nodeValue`.** Запись
   `firstChild.data = ...` уходила в expando-свойство обёртки → 0 записей вместо
   `characterData` с корректным `oldValue`.

Плюс сопутствующее: `MutationRecord.target` брался от наблюдаемого узла, а не
от мутировавшего (для subtree-наблюдателя это разные узлы); `addedNodes`/
`removedNodes` несли nid-числа вместо обёрток узлов; отсутствовал
`attributeNamespace` (checkRecords сравнивает его с `null`).

## Фикс

- `_lumen_mo_in_subtree(ancestor, nid)` — обход цепочки `_lumen_get_parent`;
  `_mo_notify` скоупит subtree-наблюдателей своим поддеревом, не-subtree — точным
  совпадением с target.
- `observe()` возвращает наблюдателя в `_mo_observers`, если его там нет.
- `_lumen_set_text_content`-wrap разветвлён по типу узла: element → `childList`
  с diff детей (до/после), text/CharacterData → `characterData` с `oldValue`.
- `data`/`nodeValue`-аксессоры на живой обёртке текст-узла (пишут через
  `_lumen_set_text_content`, откуда идёт `characterData`-нотификация).
- Запись несёт `target = _lumen_make_element(nid)` (мутировавший узел),
  обёрнутые `addedNodes`/`removedNodes`, `attributeNamespace: null`.

3 юнит-теста в `crates/js/src/dom.rs`: `mutation_observer_take_records_full_sequence`
(полный сиквенс WPT `MutationObserver-takeRecords`), `mutation_observer_reobserve_after_disconnect_delivers`,
`mutation_observer_subtree_scoped_to_target`.
