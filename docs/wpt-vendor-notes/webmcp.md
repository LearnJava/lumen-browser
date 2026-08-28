# WPT vendor notes — `webmcp`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-09 by the WPT-VENDOR backlog (`ROADMAP.md` `WPT-VENDOR-webmcp`, `docs/wpt-status.md`), scope 🚫 (out of scope — experimental proposal, https://webmachinelearning.github.io/webmcp/, no engine surface implements it). Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit hash, `LICENSE-WPT.md` copied from the sibling `webidl` category, 61 files (`META.yml`, `idlharness.https.window.js`, `declarative/` — 12 files for the `<tool>`-element declarative registration form, `imperative/` — 46 files for `navigator.modelContext.registerTool`/`getTools`/`executeTool`, `resources/helpers.js`). All but one source file are `.https.` — the single exception, `imperative/non-secure.html`, exists specifically to assert the API is *absent* on an insecure origin.

`run_report.py --all --root webmcp --recursive --processes=4` (~6:10, 50 selected ids): **1/50 harness OK, 1/1 subtests** — only `non-secure.html` executed (asserts `'modelContext' in navigator === false` on `http://`, which is trivially true since the API isn't implemented at all). All other 49 tests TIMEOUT on the already-documented TLS `UnknownIssuer` gap ([BUG-657](../../bugs/BUG-657-OPEN.md)) before reaching any assertion — zero functional signal from the runner.

A live probe (`--mcp-live-port`) checked for the class of "leaked partial implementation" defect this backlog has repeatedly found in other 🚫-scope categories (e.g. [BUG-712](../../bugs/BUG-712-OPEN.md) `navigator.gpu`, [BUG-713](../../bugs/BUG-713-OPEN.md) `HIDManager`/`HIDDevice`): `navigator.modelContext`, `navigator.modelContext.registerTool`/`.getTools`/`.executeTool`, `window.ModelContext`, and `'modelContext' in navigator` are all `undefined`/`false` — the API is completely absent, not a stub, not a bare object literal, nothing partially wired. Matches `crates/js/` grep (`webmcp`/`modelContext`/`ModelContext` — zero hits). Consistent with the existing ROADMAP scope note; no new bug filed.

## Прогон и находки (`docs/wpt-status.md`)

Вендорена целиком 2026-08-09 (коммит `35be3b44`, `tests/wpt/webmcp/`, 61 файл, `LICENSE-WPT.md` скопирован из `webidl`). Скоуп 🚫 — экспериментальное предложение (WebMachineLearning Working Group), в движке нет ни одной строчки реализации (`grep -rn "webmcp\|modelContext\|ModelContext" crates/js/src/` — 0 совпадений).

`run_report.py --all --root webmcp --recursive --processes=4` (~6:10, 50 отобранных id): **1/50 harness OK, 1/1 сабтестов**. Единственный исполнившийся тест — `imperative/non-secure.html` (не-`.https.`, проверяет отсутствие API на небезопасном origin — тривиально верно). Остальные 49 — TIMEOUT на TLS-гэпе `UnknownIssuer` ([BUG-657](../../bugs/BUG-657-OPEN.md)), нулевой функциональный сигнал.

Живая проба (`--mcp-live-port`) проверила класс дефекта «протёкшая частичная реализация», уже находившийся в других 🚫-категориях ([BUG-712](../../bugs/BUG-712-OPEN.md) `navigator.gpu`, [BUG-713](../../bugs/BUG-713-OPEN.md) `HIDManager`/`HIDDevice`): `navigator.modelContext`, `.registerTool`/`.getTools`/`.executeTool`, `window.ModelContext`, `'modelContext' in navigator` — все `undefined`/`false`. API отсутствует целиком, не заглушка и не голый объект — новый номер бага не заводился, скоуп-заметка ROADMAP.md подтверждена.
