# WPT vendor notes — `permissions-revoke`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/permissions-revoke/`, 3 файла: `META.yml`, `idlharness.any.js`, `LICENSE-WPT.md` скопирован из соседней `permissions-request`). `run_report.py --all --root permissions-revoke --recursive` (~14 с, 1 отобранный id): **0/1 harness OK**. Единственный тест `idlharness.any.html` TIMEOUT на уже задокументированном гэпе `/resources/idlharness.js`+`/resources/WebIDLParser.js` (не вендорены). Живая проба (`--mcp-live-port`) `navigator.permissions.*` показала, что и при вендоренных хелперах тест бы не прошёл: `revoke` (нестандартный, только Chromium, используется тестами WPT для сброса состояния разрешения) отсутствует вовсе — `Object.getOwnPropertyNames(navigator.permissions)` → `["query"]`. Заведён [BUG-652](../../bugs/BUG-652-FIXED.md)

## Перепрогон 2026-09-25 (BUG-652 закрыт)

`Permissions.prototype.revoke()` установлен (`crates/js/src/permissions.rs`). `run_report.py --all --root permissions-revoke --recursive`: 8/14 → **2/2 harness OK, 11/14 сабтестов**; `idlharness.any.html` зелёный целиком. Три оставшихся FAIL — `idlharness.any.worker.html`: в воркерах нет `navigator.permissions` ([BUG-1174](../../bugs/BUG-1174-OPEN.md)). Эталон `.ini` перегенерирован, `--check` — 0 регрессий.
