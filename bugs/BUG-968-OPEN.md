# BUG-968: mutating `.src` on an already-prepared `<script>` never re-runs
"prepare a script" — no second fetch, no second execution

**Статус:** OPEN
**Дата:** 2026-09-03
**Компонент:** js (`crates/js/src/shim/web_api_shim_mid.js` §"resource
tracking" — `_lumen_resource_pending`/`_lumen_resource_try_prepare`)
**Найден:** P2, WPT-RUN-6 срез 55, живой пробой

## Механизм

`_lumen_resource_pending` is the shim's whole model of the spec's per-element
"already started" flag (`web_api_shim_mid.js:8147-8154`): `createElement`/
`createElementNS` marks a freshly-minted `script`/`link`/`track`/`source`/
`style` node as pending (`_lumen_resource_track`, line 8159), and the first
time that node becomes connected, `_lumen_resource_try_prepare` (line 8842)
deletes the pending entry and calls `_lumen_script_prepare(nid)` — exactly
once, ever, per element. Nothing else in the shim ever calls
`_lumen_script_prepare` again for that nid afterwards.

That is correct for the spec's *original* "already started" gate (a script
element is prepared at most once from its initial insertion), but
[the HTML Standard changed in 2026](https://github.com/whatwg/html/pull/10188)
to also re-run "prepare a script" when a **non-parser-inserted** script's
`src` attribute is mutated *after* it already had a valid `src` — the common
"swap the bundle" pattern used by module-federation/lazy-loading code.
Nothing in Lumen implements this second trigger: `src` is a plain reflected
URL attribute (`['src', 'src', 'url']`,
`crates/js/src/shim/web_api_shim_tail_b.js:1277` and siblings) with no
side-effecting setter, and `_lumen_resource_pending` has no re-entry path for
a node whose entry was already deleted. A `<script>` element therefore
fetches and executes **at most once in its lifetime**, no matter how many
times `.src` is reassigned afterwards.

## Симптом

Confirmed live (`--mcp-live-port`, `tests/wpt/verify_slice55_gaps.py`,
2026-09-03, `main` = `d05d62fd2`):

```
harness-complete status=2 tests=1
  Mutating `src` attribute from an already-valid value does 'prepare' the script:2   (TIMEOUT)
```

`serve_wpt_like.py`'s own request log for the run shows **zero** requests for
`resources/flag-setter.js` in either spelling (`resources/flag-setter.js` or
`resources/flag-setter.js?different`) — only the test document itself plus
`testharness.js`/`testharnessreport.js` are ever fetched. The test:

1. creates a `<script type="invalid" src="resources/flag-setter.js">` and
   appends it — `type="invalid"` makes the *initial* prepare a no-op per
   spec (unsupported script type), consistent with no request yet;
2. sets `.type = ''` (now a valid classic script) and awaits `.onload`;
3. mutates `.src = 'resources/flag-setter.js?different'` — this is the step
   that is supposed to trigger a **fresh** prepare-and-fetch per the PR
   above.

Step 3 never issues a request at all, so `script.onload` never fires and the
awaited promise hangs forever — the harness only reports at its own internal
timeout, with the sole registered test stuck at TIMEOUT (subtest status `2`).

Real-world trigger:
`/html/semantics/scripting-1/the-script-element/change-src-attr-prepare-a-script.html`
— matches the WPT-RUN-5/6 corpus TIMEOUT signature for this id.

## Масштаб

Any dynamic-import/lazy-loading pattern that reuses one `<script>` element
and repoints `.src` to load a different module (rather than creating a fresh
element each time) will silently never execute past the first swap — this is
a real, if less common, alternative to the usual "one `createElement` per
load" idiom the rest of `_lumen_resource_track` already covers.

**Also the "no `src` at connection time, `.src` set for the first time
afterwards" case — not a narrower variant, the same one-shot gate.**
Corrected 2026-09-04 (WPT-RUN-6 срез 57): the note originally here claimed
this shape was "already covered by the connection-time trigger", which reads
`_lumen_resource_try_prepare`'s single call to `_lumen_script_prepare` as
running again once `src` becomes non-empty — it does not, that call already
fired once at connection (with `src` empty, so it took the inline/no-op
branch) and nothing re-enters it later. Confirmed live: `execution-timing/
023.html` appends a `<script>` with no `src`/no body via `testlib.addScript`,
then on `load` sets `.src = 'scripts/include-1.js'` and awaits `.onload` —
`tests/wpt/verify_slice57_gaps.py` measured `harness-complete status=2`
(TIMEOUT), the identical `.onload`-never-fires shape as the already-valid-src
case above, on `_lumen_script_prepare`'s own "at most once in its lifetime"
read in §Механизм. The fix below already covers this case too — it needs no
separate condition, only for the guard on *when* to re-enter
`_lumen_script_prepare` not to special-case "was previously non-empty".

## Что нужно

Give `.src` (and, per the same PR, `.type`) a side-effecting setter on
non-parser-inserted, already-connected `<script>` elements that re-enters
`_lumen_script_prepare` whenever `src` is mutated after the element's
connection-time prepare has already run — whether that earlier `src` was
empty or valid. Needs its own bookkeeping distinct from
`_lumen_resource_pending`, since that map's whole point is "prepared exactly
once" — this is deliberately a *second* (or later) legitimate prepare, not a
re-entry bug to guard against.

## Классификация WPT-RUN-6

Attributed via
`_exact_id_marker("/html/semantics/scripting-1/the-script-element/change-src-attr-prepare-a-script.html", "/html/semantics/scripting-1/the-script-element/execution-timing/023.html")`
in `tests/wpt/timeout_audit.py` (marker `script-src-mutation-not-prepared`).
