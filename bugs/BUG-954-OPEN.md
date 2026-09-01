# BUG-954 — `appendChild`/`insertBefore` не бросают `HierarchyRequestError` на цикле и вешают движок

**Статус:** OPEN
**Тип:** дефект реализованного кода — DOM-дерево и его нативные примитивы вставки уже есть; отсутствует ровно одна проверка (DOM §4.2.3 «pre-insert validity», случай «новый узел — включительный предок родителя»).
**Заведён:** 2026-09-02 (WPT-RUN-6, срез 33, живая проба `verify_slice33_gaps.py --variant dom-cycle-appendchild`)
**Область:** dom (`crates/engine/dom/src/lib.rs` — `Document::append_child`/`insert_before`/`insert_after`, единственная защита — `debug_assert!(!self.is_self_or_ancestor(...))`), js (`crates/js/src/v8_runtime/install/dom_core.rs` — `_lumen_append_child`/`_lumen_insert_before` зовут `doc.append_child`/`doc.insert_before` без какой-либо проверки со своей стороны; `crates/js/src/shim/web_api_shim_mid.js:4869` — JS-обёртка `appendChild` тоже ничего не проверяет, кроме CharacterData, BUG-325)
**Владелец:** P3.

## Симптом

`testselect2.add(opt2)` в `html/semantics/forms/the-select-element/select-add.html`
вызывает `testselect2.appendChild(opt2)`, где `opt2` — ВКЛЮЧИТЕЛЬНЫЙ ПРЕДОК
`testselect2` в разметке (`<option id=testoption><select id=testselect2>…`).
По DOM §4.2.3 такая вставка обязана бросить `HierarchyRequestError` до какой
бы то ни было мутации дерева. Движок вместо этого вешается насмерть:
живая проба показывает, что `testselect2.add(opt2)` не возвращает управление
вообще — ни исключения, ни следующей строки скрипта, ни даже `setInterval`
с периодом 500 мс (полное отсутствие тиков «страница жива»). Процесс не
падает и не паникует — в логе браузера после вызова нет ничего, кроме того,
что успело напечататься до него; движок просто замирает.

## Причина

Единственная защита от цикла — `debug_assert!` в трёх местах
`crates/engine/dom/src/lib.rs` (`append_child`, `insert_before`,
`insert_after`): `debug_assert!(!self.is_self_or_ancestor(child, parent), …)`.
`[profile.dev-release]` наследует `[profile.release]` (`inherits = "release"`)
и нигде не переопределяет `debug-assertions` (грепом по `Cargo.toml` — ноль
совпадений), поэтому в стандартной сборке, на которой и гоняется корпус,
`debug_assert!` компилируется в пустоту — проверка не выполняется вообще, не
то что не бросает JS-исключение. Нативные биндинги
(`_lumen_append_child`/`_lumen_insert_before`, `v8_runtime/install/dom_core.rs`)
зовут `Document::append_child`/`insert_before` напрямую, без собственной
проверки. JS-обёртка (`appendChild`, `web_api_shim_mid.js:4869`) проверяет
только CharacterData (BUG-325) и сразу зовёт нативную функцию.

Вставка `testselect2.add(opt2)` детачит `opt2` от его текущего родителя
(`<form>`) и делает `opt2.parent = testselect2` — но `testselect2` при этом
остаётся ребёнком `opt2` (его запись не трогали), так что за один вызов
получается настоящий двухузловой цикл `testselect2 → opt2 → testselect2 → …`.
Какой именно последующий обход дерева уходит в бесконечный цикл на этом
цикле, живая проба не различает (кандидаты: обход дерева стилей/лэйаута на
engine-thread под держащимся `Mutex<Document>`, который затем блокирует
JS-поток на следующей же нативной функции; либо сам обход где-то в JS-обёртке
после `_lumen_append_child`) — важно то, что мутация проходит без всякой
проверки корректности, и починка адресуется на уровне самой вставки, а не
обхода.

## Прямое измерение

Живая проба (`--variant dom-cycle-appendchild`, dev-release, `main` = `8b634befc`):
маркер `opt2-is-ancestor-of-testselect2-before = true` печатается (подтверждает
разметку — `opt2.contains(testselect2)` истинно ДО вызова), но
`add-threw`, `testselect2-parent-after`, `opt2-parent-after`,
`about-to-serialize`, `outerHTML-length`, `serialize-done` — ни один из этих
маркеров, идущих в скрипте СРАЗУ после `testselect2.add(opt2)`, не печатается
за 8 секунд ожидания. Контрольный вариант (`control`, только `raf`/`load`) за
то же время печатает оба маркера и 15 тиков `setInterval` — страница жива и
отзывчива. Это отличает «зависла именно эта вставка», а не «сеть/сервер
пробы».

## Кого это держит

`html/semantics/forms/the-select-element/select-add.html` — 1 id, второй из
двух тестов файла (первый, `testselect1.add(opt1)` для несвязанных узлов,
скорее всего проходит штатно, отдельно не проверялось). Ровно этот id входит
в `unclassified`-остаток WPT-RUN-6 (срез 33).

## Направление починки

Добавить проверку «включительный предок» ПЕРЕД мутацией в
`Document::append_child`/`insert_before`/`insert_after`
(`crates/engine/dom/src/lib.rs`) — либо возвращать `Result`/явный признак
ошибки оттуда, либо проверять на стороне native-биндинга
(`v8_runtime/install/dom_core.rs`) до вызова `doc.append_child`/`insert_before`
и бросать `HierarchyRequestError` в JS при совпадении. `is_self_or_ancestor`
уже существует и используется в самом `debug_assert!` — её достаточно звать
безусловно, не только под `cfg(debug_assertions)`. Не полагаться на
`debug_assert!` ни для этой проверки, ни для любой другой, где отсутствие
проверки не паникует читаемо, а вешает процесс без следа в логе — это
поведение хуже паники: паника хотя бы завершает процесс с диагностикой,
здесь же движок молча перестаёт отвечать целиком.
