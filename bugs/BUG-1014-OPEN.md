# BUG-1014 — `set_permission`/`get_computed_role`/`get_computed_label` остаются неисполненными testdriver-экшенами: нужны новая BiDi-поверхность и корреляция a11y-дерева с DOM, не трансляция payload'а

**Статус:** OPEN (ДОРАБОТКА → [WPT-RUN-13](../ROADMAP.md))
**Тип:** нереализованная функциональность двух разных семейств, не дефект реализованного кода — ведётся как задача `WPT-RUN-13` в [ROADMAP.md](../ROADMAP.md), P3 как баг не берёт.
**Заведён:** 2026-09-06 (P1, WPT-RUN-12) — выделен из [BUG-810](BUG-810-FIXED.md) при его закрытии: три самых частых экшена (`action_sequence`, `send_keys`, `delete_all_cookies`) были трансляцией уже существующего транспорта и закрыты той заявкой; эти два — нет.
**Область:** `tools/wptrunner/wptrunner/executors/executorlumen.py::_handle_action`; `crates/bidi-server/src/protocol.rs` (нет ни одного `permissions.*`-метода); `crates/engine/a11y`, `crates/driver/src/types.rs::AutomationCommand::A11yTree` (дерево есть, корреляции с DOM нет)
**Владелец:** P2 (обвязка WPT) для клиентской части; серверная часть (`permissions.*` в bidi-server, корреляция a11y↔DOM) — по объёму ближе к P1/движку, координация между дорожками на усмотрение того, кто берёт задачу.

## Срез 1 (2026-09-30, P6): `set_permission` закрыт

`permissions.setPermission` реализован: `crates/bidi-server/src/protocol.rs::permissions_set_permission`
→ `BrowserSession::set_permission` → `AutomationCommand::SetPermission` → shell →
`_lumen_permission_set` в шиме `crates/js/src/permissions.rs`. Состояние — процесс-глобальное
(`v8_runtime::set_global_permission_override`), переживает навигацию, `query()` его читает и
шлёт `change`. Неизвестное имя/состояние — `invalid argument`. Origin принимается, но состояние
им не разделяется (живёт один origin страницы). Executor: `_action_set_permission`
(`set_permission` и `bidi.permissions.set_permission`). **Остаётся** `get_computed_role`/`get_computed_label`.

## Срез 2 (2026-09-30, P6): `get_computed_role`/`get_computed_label` реализованы

Корреляция не понадобилась: `AXNode.node_id` — это id DOM-узла. Расширение Lumen
`lumen.getComputedA11y {selectors}` (`protocol.rs::lumen_get_computed_a11y`) →
`BrowserSession::computed_a11y` → `AutomationCommand::ComputedA11y` → shell
(`automation_computed_a11y`: цепочка селекторов через `query_all_within` + `shadow_root_of`)
→ `lumen_a11y::computed_role_and_name` (узел ищется в построенном дереве; вне дерева, напр.
под `aria-hidden`, роль/имя считаются отдельно). Executor: `_action_get_computed_a11y`.
Проверено: юнит-тест a11y, clippy. **Не проверено** сквозным прогоном WPT (`accname`) на живом
окне — без него баг не закрыт.

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
