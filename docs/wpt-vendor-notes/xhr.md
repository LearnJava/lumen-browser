# WPT vendor notes — `xhr`

## Vendoring (`tests/wpt/VENDOR.md`)

Test category, added 2026-08-18 by the WPT-VENDOR backlog (`ROADMAP.md`
`WPT-VENDOR-xhr`, `docs/wpt-status.md`), scope ⬜ (in scope). Confirmed
before vendoring: `XMLHttpRequest` is a real, working implementation
(`crates/js/src/xhr.rs`, its own `XHR_SHIM` reusing the same native
fetch bindings as `fetch()`), not a stub.

Same pinned upstream commit `35be3b44`, `git sparse-checkout add xhr` at
that commit, `LICENSE-WPT.md` copied from a sibling category — 443 files
(442 upstream + license), 345 glob ids (matched by the actual run count).
Cheap predictors across the board: 0 `name="variant"` hits, 1
`testdriver.js` hit (single file), 0 `.https.` files. Out-of-category deps
(`/common/utils.js`, `/cors/support.js`,
`/permissions-policy/resources/permissions-policy.js`) were already
vendored by earlier categories. Confirmed cheap in practice: 14 min 19 s
wall-clock, single process.

### Run result

`run_report.py --all --root xhr --recursive` (14 min 19 s, single
process): **236/345 harness OK, 157/1244 subtests passed** — real signal,
the vast majority of files actually execute (no HTTPS-port gap, no
testdriver wall).

### Dominant finding: [BUG-780](../../bugs/BUG-780-OPEN.md)

`XMLHttpRequest.prototype.open` (`crates/js/src/xhr.rs:216-236`) takes the
`url` argument completely literally (`this._url = String(url);`) — no
base-URL resolution step at all, unlike `fetch()`/`Request` (fixed by
[BUG-347](../../bugs/BUG-347-FIXED.md)/[BUG-370](../../bugs/BUG-370-FIXED.md)
via `_url_resolve(url, _lumen_document_base_url())`). `send()` forwards
`self._url` unchanged into the same native fetch bindings `fetch()` uses,
so it fails identically once it reaches `Url::parse` in
`crates/network/src/lib.rs`. `xhr.rs` is a separate shim module (its own
`rt.eval(XHR_SHIM)`, not part of `WEB_API_SHIM` in `dom.rs`) — the sixth
independent site of the BUG-346/347/359/362/370 family, and it did not
inherit BUG-347's fix because the fix landed in a sibling file.

Live-probe confirmation (`--mcp-live-port`, page served over
`http://127.0.0.1:.../xhr_probe.html`, not `file://` — see the CLAUDE.md
gotcha on `fetch`/XHR being no-ops from `file://`/headless):

```
XHR open() relative URL, ._url after open: {"url_after_open":"resources/foo.json"}
XHR open() absolute-path URL, ._url after open: {"url_after_open":"/xhr_probe.html"}
fetch/Request relative URL resolution (already-fixed sibling):
  {"fetch_resolved_url":"http://127.0.0.1:18471/resources/foo.json"}
```

Weight: **739 lines** of `fetch error: invalid url: invalid url: missing
scheme: "..."` in the run log — by far the dominant failure class, an
order of magnitude above the next contributor. Examples:
`"resources/well-formed.xml"`, `"resources/utf16-bom.json"`,
`"resources/delay.py?ms=1000"`, `"/common/blank.html?pipe=trickle(d1)"` —
both script-relative and root-relative forms.

### Everything else traces to already-known gaps

- **`data:` scheme unsupported by the network layer at all**
  (`crates/network/src/lib.rs:267`, explicit comment: "Bad scheme
  (`ftp://`, `data://`, `file://`) — early exit"). 17 lines of `fetch
  error: unsupported scheme: data` in the log — a separate, deliberate
  Phase-0 limitation, not caused by BUG-780. Affects `json.any.html`'s
  first subtest (`xhr.open("GET", "data:,...")`) among others. Not filed
  as a new bug — pre-existing, documented scope gap.
- **`window.open`/multi-window family** (`open-url-multi-window*`,
  `open-url-worker*`, `open-url-redirected-*-origin.htm`) — the "second
  barrier" of [BUG-359](../../bugs/BUG-359-FIXED.md) already on record:
  `window.open` returns a stub without a real `opener`/`postMessage`
  round-trip, so any test waiting on a popup's response TIMEOUTs.
- **`xmlhttprequest-timeout-reused.html` and neighbors** — browsing
  context reuse serving a stale document's results, the same class as
  [BUG-380](../../bugs/BUG-380-FIXED.md) (`executorlumen.py` doesn't
  validate `result_url == test.url`; here it's the engine side navigating
  but the harness catching a still-live previous test).

No second engine bug beyond BUG-780 was root-caused this session — the
739-line dominant signal made further per-file triage low-value until
BUG-780 itself is fixed and the category is re-run.

## Прогон и находки (`docs/wpt-status.md`)

Скоуп ⬜ подтверждён — `XMLHttpRequest` реализован по-настоящему
(`crates/js/src/xhr.rs`, свой `XHR_SHIM`, те же нативные fetch-биндинги,
что `fetch()`), не заглушка. Вендорена целиком 2026-08-18 (коммит
`35be3b44`, `tests/wpt/xhr/`, 443 файла, 345 id, дёшево по всем
предикторам: 0 variant, 1 testdriver-файл, 0 `.https.`).

`run_report.py --all --root xhr --recursive` — 14 мин 19 с, **236/345
harness OK, 157/1244 сабтестов**. Найден [BUG-780](../bugs/BUG-780-OPEN.md):
`XMLHttpRequest.open()` берёт `url` буквально (`this._url = String(url)`),
без единого шага резолюции против document base — шестой независимый
сайт семейства BUG-346/347/359/362/370, не унаследовавший фикс BUG-347
(`fetch()`), поскольку `xhr.rs` — отдельный шим, не часть `WEB_API_SHIM`.
739 строк `missing scheme` — доминирующий класс отказов на порядок выше
следующего по весу. Живая проба подтверждает напрямую: `fetch()`/`Request`
на той же странице резолвят корректно, `XMLHttpRequest.open()` — нет.

Остальное — уже известные гэпы: `data:`-схема не поддержана сетевым слоем
вовсе (`crates/network/src/lib.rs:267`, отдельный и заведомый Phase-0
пробел, не этот баг), второй барьер [BUG-359](../bugs/BUG-359-FIXED.md)
у `window.open` (нет реального `opener`), и класс
[BUG-380](../bugs/BUG-380-FIXED.md) (переиспользуемый browsing context)
у `xmlhttprequest-timeout-reused.html` и соседей. Второй самостоятельный
баг в этой сессии не root-caused — 739-строчный доминирующий сигнал делает
дальнейшую построчную триажировку малоценной до фикса BUG-780 и повторного
прогона.
