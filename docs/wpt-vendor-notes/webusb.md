# WPT vendor notes — `webusb`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-webusb`, `docs/wpt-status.md`), scope 🚫 ("аппаратный API (USB)" —
hardware API). Same pinned commit `35be3b44`, `git sparse-checkout add` at the
same commit hash, `LICENSE-WPT.md` copied from a sibling category, 41 files
(32 glob ids — 6 `-manual.https.html` pages excluded, no variant fan-out).

Confirmed the ROADMAP note's scope call before vendoring — grepped
`crates/js/src/webusb.rs`: the whole file is a Phase-0 JS shim.
`navigator.usb` is a real `USBManager` (extends `EventTarget`), `getDevices()`
resolves to `[]`, but `requestDevice()` always rejects `NotSupportedError`
and every `USBDevice` method (`open`/`close`/`selectConfiguration`/
`claimInterface`/`transferIn`/`transferOut`/…) throws the same. No Rust-side
USB backend exists anywhere in the workspace. Scope call stands: 🚫, a
Phase-0-only surface with no reachable success path — every test that gets
far enough to call `requestDevice()` necessarily fails.

`run_report.py --all --root webusb --recursive` (~2 min 52 s, venv python):
**2/32 harness OK, 0/15 subtests**. 30 of 32 ids are `.https.` (WebUSB
requires a secure context by spec) and hit the pre-existing, already-documented
TLS-trust gap from WPT-RUN-2 (`tests/wpt/certs/README.md`, `UnknownIssuer`,
also tracked as [BUG-657](../../bugs/BUG-657-OPEN.md)) before touching
`webusb.rs` at all — not a category finding.

The 2 non-`.https.` ids that did run produced two FAILs, neither a new
defect:

- `insecure-context.any.html`: `"usb" should not be present on navigator in
  an insecure context` fails — `navigator.usb` is installed unconditionally
  by `Object.defineProperty(navigator, 'usb', {...})`
  (`crates/js/src/webusb.rs:194-198`), with no `isSecureContext` branch.
  This reconfirms the already-open umbrella bug
  [BUG-765](../../bugs/BUG-765-OPEN.md) ("no `[SecureContext]`-tagged API is
  gated by `window.isSecureContext` — the surface is installed by
  unconditional assignment regardless of context") — not a new number, one
  more instance of a documented, already-tracked class.
- `usb-supported-by-permissions-policy.html`: `document.permissionsPolicy
  .features()` does not list `"usb"` — expected, not a bug: after
  [BUG-361](../../bugs/BUG-361-FIXED.md)'s fix `features()` correctly
  reports the engine's real supported-feature registry
  (`_ppSupported`), and WebUSB genuinely has no Permissions-Policy
  integration to report (Phase-0 stub, confirmed above) — omitting `"usb"`
  is the *correct* behaviour for a feature the engine does not implement.

No new `BUG-NNN` filed.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 «аппаратный API (USB)» подтверждён точно перед вендорингом (грепом
`crates/js/src/webusb.rs` — весь файл Phase-0: `navigator.usb` реален
(`USBManager`), но `requestDevice()` и все методы `USBDevice` кидают/реджектят
`NotSupportedError`; Rust-бэкенда USB в воркспейсе нет). Вендорена целиком
2026-08-18 (коммит `35be3b44`, `tests/wpt/webusb/`, 41 файл, 32 id, 6
`-manual.https.html` исключены, без variant-фан-аута). `run_report.py --all
--root webusb --recursive` (~2:52) — **2/32 harness OK, 0/15 сабтестов**.
30/32 — `.https.`-гэп TLS `UnknownIssuer` ([BUG-657](../bugs/BUG-657-OPEN.md)),
не находка категории. Два исполнившихся non-`.https.` теста дали два FAIL —
оба не новые баги: `insecure-context.any.html` переподтверждает уже открытый
[BUG-765](../bugs/BUG-765-OPEN.md) (`navigator.usb` ставится безусловно, без
гейта по `isSecureContext`); `usb-supported-by-permissions-policy.html`
ожидаемо не находит `"usb"` в `features()` — WebUSB не имеет
Permissions-Policy интеграции (Phase-0), это корректное поведение, а не
регресс [BUG-361](../bugs/BUG-361-FIXED.md). Новый BUG-NNN не заводился.
