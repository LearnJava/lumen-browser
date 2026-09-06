# BUG-1014 — `set_permission`/`get_computed_role`/`get_computed_label` остаются неисполненными testdriver-экшенами: нужны новая BiDi-поверхность и корреляция a11y-дерева с DOM, не трансляция payload'а

**Статус:** OPEN (ДОРАБОТКА → [WPT-RUN-13](../ROADMAP.md))
**Тип:** нереализованная функциональность двух разных семейств, не дефект реализованного кода — ведётся как задача `WPT-RUN-13` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт.
**Заведён:** 2026-09-06 (P1, WPT-RUN-12) — выделен из [BUG-810](BUG-810-FIXED.md) при его закрытии: три самых частых экшена (`action_sequence`, `send_keys`, `delete_all_cookies`) были трансляцией уже существующего транспорта и закрыты той заявкой; эти два — нет.
**Область:** `tools/wptrunner/wptrunner/executors/executorlumen.py::_handle_action`; `crates/bidi-server/src/protocol.rs` (нет ни одного `permissions.*`-метода); `crates/engine/a11y`, `crates/driver/src/types.rs::AutomationCommand::A11yTree` (дерево есть, корреляции с DOM нет)
**Владелец:** P2 (обвязка WPT) для клиентской части; серверная часть (`permissions.*` в bidi-server, корреляция a11y↔DOM) — по объёму ближе к P1/движку, координация между дорожками на усмотрение того, кто берёт задачу.

## Почему это доработка, а не остаток той же починки

`probe-method.md §8`: доработка требует функциональности, которой нет вовсе
(не сломана), **и** размера семейства/модели состояния, а не одной строки.
Оба пункта здесь выполнены для обеих позиций:

- **`set_permission`.** `grep -rn "permissions\." crates/bidi-server/src/protocol.rs`
  даёт ноль совпадений — ни `permissions.setPermission`, ни любой другой метод
  этого BiDi-модуля не диспетчеризуется вообще. У Lumen нет и модели разрешений
  как таковой (`navigator.permissions.query()` не читает никакого состояния,
  которое `set_permission` мог бы менять) — значит фикс не «добавить один
  обработчик», а спроектировать состояние permission-descriptor'ов и подключить
  его и к BiDi, и к `navigator.permissions`.
- **`get_computed_role`/`get_computed_label`.** Accessibility-дерево
  реализовано (`crates/engine/a11y`, `AutomationCommand::A11yTree`,
  `BrowserSession::query_a11y`/`query_a11y_all` в `lumen-driver`, `AxQuery::Role`
  ищет узел ПО роли+имени), но ни один путь не решает обратную задачу — по
  DOM-элементу (то, что дают `params["selectors"]`) найти соответствующий
  `AXNode` и прочитать его роль/имя. Нужна модель корреляции (общий id между
  деревьями, либо геометрический lookup по прямоугольнику, как у
  `_resolve_element_center`), не один вызов.

## Направление (не предписание)

1. `set_permission`: спроектировать permission-state (по origin, как в
   `PermissionsState` большинства реализаций) в `lumen-bidi-server` или
   отдельном крейте, подключить `permissions.setPermission`
   (WebDriver BiDi permissions module) и сверить с тем, что реально читает
   `navigator.permissions.query()` — иначе экшен «успешен», а наблюдаемое
   поведение не меняется, то есть тест зеленеет ложно.
2. `get_computed_role`/`get_computed_label`: самый дешёвый путь — не общий id
   между деревьями (это переделка `crates/engine/a11y`), а geometry-based
   lookup, аналогичный `_resolve_element_center`: получить прямоугольник
   элемента по `selectors` (уже есть), затем найти `AXNode`, чей прямоугольник
   его содержит, через `AutomationCommand::A11yTree`. Достаточно для большинства
   `get_computed_role`/`label`-тестов (`accname`, часть `css/selectors`
   `focus-visible`), но не для элементов без собственного layout-прямоугольника
   (`display: contents`, некоторые ARIA-роли) — это отдельный хвост.

## Как проверить фикс

Тот же метод, что WPT-RUN-12: `run_smoke.py`/`run_report.py` на тестах,
использующих `test_driver.set_permission`/`get_computed_role`/
`get_computed_label` (например `permissions/resources/*`,
`accname/computedname` в вендоренном корпусе), плюс `timeout_audit.py --json`
— механизм `testdriver-action-unimplemented` должен и дальше уменьшаться.
