# WPT vendor notes — `window-management`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-window-management`, `docs/wpt-status.md`), scope 🚫 (out of
scope, same criterion already applied to the sibling `screen-details`
category — multi-monitor OS integration is outside a lightweight reader
browser's product scope, independent of whether a stub exists in code).
Same pinned commit `35be3b44`, `git sparse-checkout add` at the same commit
hash, `LICENSE-WPT.md` copied from a sibling category, 10 files (5 glob ids,
all `.tentative.https.`, no variant fan-out).

Predictor check: 4 of 5 files pull `testdriver.js`, which by the share rule
would suggest a SKIP wall — but (per the `font-access`/`fledge` precedent
already on record) the actual run showed the predictor doesn't fire here
either: all 5 hit the TLS gap before SKIP or testdriver logic is ever
reached.

`run_report.py --all --root window-management --recursive` (~5 min, venv
python): **0/5 harness OK, 0/0 subtests**. All 5 navigations failed with
`network error: TLS handshake: invalid peer certificate: UnknownIssuer` —
the pre-existing, already-documented WPT-RUN-2 residual
(`tests/wpt/certs/README.md`: Lumen's TLS client validates against the real
Mozilla root list, so the pregenerated self-signed WPT cert is rejected by
design; "reaches and reports", not "passes", was WPT-RUN-2's stated DoD).

**Citation correction, found while writing this note:** at least 16 prior
WPT-VENDOR merge commits (starting from at least `web-share`, most recently
`webtransport`/`webxr`) cite `BUG-657` as the source of this TLS gap.
`bugs/BUG-657-OPEN.md` is actually titled `ServiceWorkerRegistration` global
class never defined in production V8 — a completely unrelated finding from
`WPT-VENDOR-push-api` (2026-08-05); `git log --follow` on that file shows a
single creating commit, so this isn't a renumber collision, just a citation
that was never verified and got copy-pasted forward. The `websockets` row
(2026-08-18) already phrases this correctly without a bug number
("уже задокументированный TLS-гэп (`UnknownIssuer`)"). This note and the
corresponding `ROADMAP.md`/`docs/wpt-status.md`/`VENDOR.md` rows for
`window-management` cite `tests/wpt/certs/README.md` instead. The existing
16 mis-citations were not retroactively fixed — that's a corpus-wide sweep
outside a single category's scope (P5 doc-drift territory), not attempted
here.

Since the run gave zero signal, `crates/js/src/window_management.rs` was
read directly (pure self-contained JS shim, no native binding involved, so
source reading is ground truth here — no live-page divergence possible).
It implements a real, documented Phase-0 stub: `screen.isExtended` always
`false`, `navigator.getScreenDetails()` resolves with a single
`ScreenDetailed` mirroring the current screen. `navigator.getScreenDetails()`
never calls into `navigator.permissions` at all — `permissions.rs`'s static
table correctly answers `denied` for `'window-management'`, but the API
itself does not check that state before resolving successfully. This is a
reconfirmation of the already-open [BUG-667](../../bugs/BUG-667-OPEN.md)
("`navigator.getScreenDetails()` never checks permission state or user
activation"), originally filed against this same file via the sibling
`screen-details` category (2026-08-05) — not a new number.

No new `BUG-NNN` filed.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп 🚫 подтверждён (тот же критерий, что `screen-details`) — мульти-монитор
ОС-интеграция вне продуктового скоупа лёгкого браузера-читалки. Вендорена
целиком 2026-08-18 (коммит `35be3b44`, `tests/wpt/window-management/`, 10
файлов, 5 id, все `.tentative.https.`). `run_report.py --all --root
window-management --recursive` (~5 мин) — **0/5 harness OK**, все 5 TIMEOUT
на TLS-гэпе `UnknownIssuer` (`tests/wpt/certs/README.md`, WPT-RUN-2 residual).

Уточнение цитаты: минимум 16 предыдущих мерж-коммитов WPT-VENDOR (начиная как
минимум с `web-share`) ссылаются на этот же TLS-гэп как [BUG-657]
(../bugs/BUG-657-OPEN.md) — но этот номер на самом деле про
`ServiceWorkerRegistration`, к TLS отношения не имеет; ошибка копировалась
без проверки. Не переисправлялось задним числом (объём вне одной категории).

`crates/js/src/window_management.rs` — реальный Phase-0 шим, прочитан
напрямую (прогон сигнала не дал): `getScreenDetails()` резолвится безусловно
и никогда не читает `navigator.permissions.query({name:'window-management'})`
(которое честно отвечает `denied`) — переподтверждение уже открытого
[BUG-667](../../bugs/BUG-667-OPEN.md), заведённого на той же заглушке через
сестринскую категорию `screen-details`. Новый BUG-NNN не заводился.
