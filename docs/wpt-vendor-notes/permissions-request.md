# WPT vendor notes — `permissions-request`

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-05 (коммит `35be3b44`, `tests/wpt/permissions-request/`, 3 файла: `META.yml`, `idlharness.any.js`, `LICENSE-WPT.md` скопирован из соседней `permissions-policy`). `run_report.py --all --root permissions-request --recursive` (~14 с, 1 отобранный id): **0/1 harness OK**. Единственный тест `idlharness.any.html` TIMEOUT на уже задокументированном гэпе `/resources/idlharness.js`+`/resources/WebIDLParser.js` (не вендорены). Живая проба (`--mcp-live-port`) `navigator.permissions.*` показала, что и при вендоренных хелперах тест бы не прошёл: `request`/`requestAll` (WICG Permissions Request API) отсутствуют вовсе — `Object.getOwnPropertyNames(navigator.permissions)` → `["query"]`, вызов бросает `TypeError`. Заведён [BUG-650](../../bugs/BUG-650-FIXED.md). Попутно при настройке живой пробы найден и заведён [BUG-651](../../bugs/BUG-651-FIXED.md): `file://`-URL в качестве начального CLI-аргумента (`--dump-layout`/`--mcp-live-port <src>`/…) не грузится (`os error 123` на Windows) — `PageSource::from_arg` не снимает схему `file://`, в отличие от `page_source_for_automation_url`, используемого для `navigate`-команд

## Перепрогон 2026-09-25 (BUG-650 закрыт)

`Permissions.prototype.request()` установлен (`crates/js/src/permissions.rs`). `run_report.py --all --root permissions-request --recursive`: **2/2 harness OK, 11/14 сабтестов**; `idlharness.any.html` зелёный целиком. Три оставшихся FAIL — `idlharness.any.worker.html`: в воркерах нет `navigator.permissions` ([BUG-1174](../../bugs/BUG-1174-OPEN.md)). Эталон `.ini` перегенерирован, `--check` — 0 регрессий.
