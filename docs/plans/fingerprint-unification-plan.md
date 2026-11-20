# Plan: fingerprint unification — one JS native registry, one Chrome 157 manifest, honest identity

**Status:** C done 2026-11-19 ([BUG-1832](../../bugs/BUG-1832-FIXED.md), branch
`fingerprint-identity-switch`); A done 2026-11-20 (branch `fingerprint-js-registry`,
ledger `runs/2026-11-19-native-api-{lumen,chrome}.json` — 0 leaks in page/dedicated/shared);
B done 2026-11-20 (branch `fingerprint-manifest`, manifest `crates/network/src/http/chrome_manifest.rs`,
self-check `runs/2026-11-20-peet-*.json`) · **Date:** 2026-11-19
**Owner:** next session (role/slot assigned by the manager; claim tasks before coding)
**Evidence:** [ADR-034 §1](../decisions/ADR-034-site-compatibility-strategy.md), A/B matrix
[2026-10-15](../perf/runs/2026-10-15-ab-matrix.md) (533 sites, token refusals 0.19 %, no must-work),
[BUG-1826](../../bugs/BUG-1826-FIXED.md) (fixed P6 + Devin), [BUG-1829](../../bugs/BUG-1829-FIXED.md) (fixed
2026-11-20 — fingerprint divergences).

This plan covers three workstreams. They are independently shippable and must go **C → A → B**:

| # | Workstream | Size | Why this order |
|---|---|---|---|
| C | Default identity `chrome_lumen` + fiverr-com compat record | S (0.5 day) | One line, data-backed by ADR-034; the manifest in B must include it |
| A | One JS native-function registry (page + workers) | M (1.5–2 days) | Kills the two overlapping mechanisms and the toString leak class |
| B | Single Chrome 157 fingerprint manifest | L (2–3 days) | Largest refactor; absorbs A's findings and C's identity data |

**Non-goals** (do not do unless the owner asks): pure `Lumen/x.y` UA profile with its own
`HttpProfile`/H2 settings/rustls hello; replacing BoringSSL; per-site identity records beyond
fiverr.com; removing rustls for h3/DoT.

---

## 0. How to use this document

- Each workstream has its own acceptance criteria. A failed criterion is a stop-and-report, not a
  retry with lower expectations.
- Every behavioural claim about "works" must be backed by an artifact in the repo (test, JSON run
  record, journal entry) — not by "ran it once locally".
- If a step contradicts what is in the tree, trust the tree and report the drift; line numbers below
  are as of 2026-11-19 and will move.

## 1. Session protocol (do this first)

1. `git pull -q origin main` in your own pool slot (`.claude/worktrees/p<N>-work`), never in the
   shared root.
2. Find or create task ids in `STATUS-PN.md` / the roadmap; suggested names if none exist:
   `FINGERPRINT-IDENTITY-SWITCH`, `FINGERPRINT-JS-REGISTRY`, `FINGERPRINT-MANIFEST`.
   Claim each with `bash scripts/task-claim.sh <N> <task-id>` (Git bash, not WSL — see traps).
3. Read before touching code:
   - `crates/js/CLAUDE.md` (required for WS-A)
   - `AGENTS.md`, `docs/commands.md`, `docs/git-workflow.md`, `docs/doc-sync.md`
   - `bugs/BUG-1826-FIXED.md`, `bugs/BUG-1829-FIXED.md`
   - `docs/invariants.md` (no layer cycles)
4. Traps that bite unconditionally:
   - PowerShell has no `tail`/`grep`/`sed`; use `Select-Object -Last`, `Select-String`, `-replace`.
   - `bash` resolves to WSL; run repo scripts as
     `& "C:\Program Files\Git\bin\bash.exe" scripts/...`.
   - Never round-trip a repo markdown file through Python text mode (CRLF/pointer shifts); use
     `newline=''` on both read and write, or the edit tools.
   - Check `git status --short` before the first `git add`; a fresh worktree can have the whole repo
     staged as deleted.
   - sccache must be ≥ 0.17.0 or every `rustc` dies with `0xc0000409`.
   - `python` may resolve to a Windows Store stub — use `py -3` when `python --version` fails
     (verified on the owner's machine, 2026-11-19).
5. Gate discipline: `cargo check` while iterating; once before the closing commit
   `cargo clippy -p <touched crates> --all-targets -- -D warnings` + targeted tests. Commit message
   in Russian, explicit staged paths, no `git add -A`, no force-push. One task = one branch =
   `merge --no-ff` into `main`.
6. This plan's work moves no pixels. Graphic tests are required only if a change unexpectedly touches
   rendering; if in doubt, run `python graphic_tests/run.py --continue-on-fail` and say so.

---

## 2. Current state (verified 2026-11-19)

### 2.1 JS layer — two overlapping native-masking mechanisms

| Mechanism | File / call site | Semantics |
|---|---|---|
| WeakMap facade | `crates/js/src/shim/native_tostring_shim.js`, spliced **last** into the page shim program (`crates/js/src/dom.rs:670`) and the worker shim program (`dom.rs:696`); re-evaluated with a fresh WeakMap at the end of `install_dom` (`crates/js/src/v8_runtime.rs:1425`) | Marks getters/setters as `function get x() { [native code] }`, walks depth 4, exposes `_lumen_tostring_mark` for functions created after install (`frame_bridge.rs:3181`) |
| Set snapshot | `crates/js/src/shim/native_mask_shim.js`, evaluated **once**, main realm only (`v8_runtime.rs:1434`) | Reachability sweep from `globalThis` (two passes: data props, then getters), then overrides `Function.prototype.toString` |

Consequences observed today:

- The page realm has up to three chained `toString` overrides (splice-time facade, re-eval facade,
  Set mask). Each captures the previous one as `originalToString`, so the chain grows a link per
  install; worker realms get only the WeakMap facade, no Set mask — page and worker coverage differ.
- Coverage depends on where the cutoff lands. Shims that create functions **lazily on first call**
  after the last sweep still leak their source; the comment in `native_tostring_shim.js:14-17`
  admits this for battery/audio-style lazy shims.
- `Object.prototype.toString` / `@@toStringTag` are not handled by either mechanism.
- Tests pinning current behaviour: `crates/js/src/dom/tests/mod.rs:24`
  (`web_api_shim_splices_its_parts_in_source_order`), `dom/tests/v8_native_mask.rs`,
  `dom/tests/v8_native_tostring.rs`.
- The BUG-378 internal-globals seal (`v8_runtime.rs:1450+`) hides `_lumen_*` globals from
  enumeration and freezes function-valued ones; anything new must survive that seal.

### 2.2 Network layer — "Chrome 157" spread across at least seven places

| Layer | File | Notes |
|---|---|---|
| UA + Client Hints | `crates/core/src/identity.rs`, `crates/network/src/http/client_hints.rs` | `chrome_major!` macro is the single source of the major; brands derive from the UA |
| Header order/values | `crates/network/src/http/headers.rs` | Chrome 157 order incl. `Sec-Fetch-*` before `Accept-Encoding` (BUG-1826 note at :781) |
| H2 SETTINGS | `crates/network/src/http/h2_settings.rs:43-60` | Measured via `tls.peet.ws/api/all` (akamai `1:65536;2:0;4:6291456;6:262144`) |
| TLS ClientHello | `crates/network/src/tls/boring.rs:38-56` | BoringSSL cipher list, sigalgs, groups, ALPN |
| TLS reference constants | `crates/network/src/tls/fingerprint.rs` | Still labelled "Chrome 130" reference — a drift marker |
| `Accept-Encoding` + decoders | derived from registration order in `crates/network/src/lib.rs:4153-4160`; registered at 7 sites: `crates/shell/src/page_source.rs:280,436`, `resource_base.rs:276`, `download.rs:526`, `update.rs:279`, `crates/driver/src/session.rs:283`, `winit_session.rs:954` | The header value is whatever order each client happened to register |
| Header-set artifacts | `docs/perf/header-sets/lumen-chrome_unbranded.json`, `docs/perf/journal.md:730` | Baseline to diff against after B |

---

## 3. Workstream C — switch the default identity to `chrome_lumen`

ADR-034 §1 already sanctions the switch ("the default may move to Chrome grammar + trailing
`Lumen/x.y`, fiverr-com gets a `chrome_unbranded` record") because the 2026-10-15 A/B met the
≤0.5 % threshold. Current code still defaults to `ChromeUnbranded`
(`crates/core/src/identity.rs:66`, `LUMEN_TOKEN` starts `false` at `:135`).

### 3.1 Code changes

1. `crates/core/src/identity.rs`
   - Move `#[default]` from `ChromeUnbranded` to `ChromeLumen`; update both doc comments.
   - `static LUMEN_TOKEN: AtomicBool = AtomicBool::new(true);`
   - Update the `identity_variant()` doc to "`IdentityVariant::default()` until set".
   - Test `variant_parses_both_spellings_and_rejects_the_rest` asserts the old default — flip it.
2. `crates/network/src/http/headers.rs`
   - `test_h2_fingerprint_headers_are_lowercase_and_drop_connection_headers` asserts
     `user-agent == CHROME_USER_AGENT`; it now receives `CHROME_LUMEN_USER_AGENT`.
   - `test_chrome_sends_sec_ch_ua_on_h1_and_h2`: Chrome/Strict now contain `Lumen` in UA and brand;
     split Edge out and keep asserting Edge never carries the token.
3. `crates/network/src/http/client_hints.rs`
   - `test_client_hints_headers_chrome` asserts `!contains("Lumen")`; invert and keep the brand/UA
     no-half-state intent.
4. `crates/shell/src/config.rs`
   - Header doc example and text: default is `chrome_lumen`; `chrome_unbranded` removes both token
     and brand.
   - `identity_key_parses_and_env_wins`: default and the `firefox_win` fallback now resolve to
     `ChromeLumen`.
5. `crates/update-manifest/assets/compat_pack.json` — first real record (pack revision 1):
   ```json
   {
     "id": "fiverr-com-identity-unbranded",
     "root_cause": "gate-antibot-ua-token",
     "matches": ["fiverr.com"],
     "site_class": "general",
     "action": { "kind": "identity", "profile": "chrome_unbranded" },
     "bug": "BUG-1832",
     "review_by": "2027-02-19",
     "tests": ["crates/update-manifest/src/compat_pack/tests.rs"]
   }
   ```
   Extend `embedded_base_copy_is_valid` (`crates/update-manifest/src/compat_pack/tests.rs:288`) to
   assert `fiverr.com` / `www.fiverr.com` → `ChromeUnbranded`, other hosts → `None`.
6. File `bugs/BUG-1832-FIXED.md` (record the A/B evidence, fix, `review_by`) and append the row to
   `BUGS-FIXED.md` using the table format.
7. Doc sync in the same commit: `CAPABILITIES.md` (identity paragraph, ~line 239),
   `docs/automation.md` (`LUMEN_IDENTITY` row, ~line 205), ADR-034 — add a dated **Amendment**
   line in the Status section, do not rewrite the history below it.

### 3.2 Verification

| Check | How | Pass criterion |
|---|---|---|
| Unit | `cargo test -p lumen-core -p lumen-update-manifest`, `cargo test -p lumen-network --lib http`, `cargo test -p lumen-shell --bin lumen config::` | All green |
| Wire echo | `scripts/header_echo_server.py` + `lumen.exe --dump-source http://127.0.0.1:<port>/` | UA ends `Lumen/<pkg version>`, `sec-ch-ua` carries `"Lumen"`, everything else byte-identical to the `chrome_unbranded` arm |
| JS parity | BiDi/MCP eval on a live page: `navigator.userAgent`, `navigator.userAgentData.brands`, `WorkerNavigator.userAgent` | Same token/brand as HTTP; workers and iframes agree |
| Per-site record | Echo server on `fiverr.com`-matched pack logic — unit test is authoritative; live check optional | `identity_for_host` resolves as in 3.1(5) |
| Challenge A/B | `python scripts/captcha_stand.py --arm lumen --route <label> --label default` and the same with `--env LUMEN_IDENTITY=chrome_unbranded` on `docs/perf/corpus-captcha.tsv` | GPR of the default arm is not worse than the tokenless arm beyond the 0.5 % threshold; record both JSONs under `docs/perf/runs/` and a `docs/perf/journal.md` entry |
| Live challenge | `lumen.exe --maximized https://claude.ai` (the BUG-1826 repro), fresh profile | Challenge page completes without an engine-thread stall; no `[gate] … подряд≥2` loop |
| fiverr.com | `lumen.exe --maximized https://fiverr.com` with the pack active | No PerimeterX 403 wall |

**Acceptance C:** all code/tests above pass; the two live checks have artifacts; docs updated;
revert path documented (`identity = "chrome_unbranded"` or `LUMEN_IDENTITY=chrome_unbranded`).

**Risk:** the A/B evidence is from 2026-10-15 (Chrome 153 era) — the live checks above are the
re-validation. If the default arm regresses, stop and report instead of shipping the flip.

---

## 4. Workstream A — one native-function registry per realm

### 4.1 Target design

Replace the two mechanisms with **one registry + one facade + one final sweep per realm**:

- `crates/js/src/shim/native_registry_shim.js` (new, replaces `native_tostring_shim.js` and
  `native_mask_shim.js`):
  - `_lumen_native_registry` (closure-held `WeakMap`/`Set`) with:
    - `register(fn, kind)` — `kind` ∈ `function | getter | setter | toStringTag`;
    - `registerTree(root, depth)` — the existing reachability sweep (data props, getters on the
      window/prototype chain, prototypes/statics), budget-capped;
    - `textFor(fn)` — Chrome-format text: `function name() { [native code] }`,
      `function get x() { [native code] }`, `function set x() { [native code] }`.
  - One override of `Function.prototype.toString` per realm, installed once, falling back to the
    original captured at install time. Guard against double-install (module flag on the global).
  - Hidden hook for functions created **after** the sweep: `_lumen_native_register(fn, kind)`
    (non-enumerable, `writable/configurable`, survives the BUG-378 seal like the current
    `_lumen_tostring_mark`). Rename `_lumen_tostring_mark` call sites (`frame_bridge.rs:3181`) or
    keep it as an alias for one release — pick one, do not leave both.
- Lifecycle:
  1. Page realm: the shim program spliced at `dom.rs:670` installs the facade early (so nothing
     before it can leak through a stale `toString` reference) but performs **no** sweep.
  2. All `install_*_v8` modules finish (`v8_runtime.rs` up to the BUG-1811 preload point).
  3. **One** sweep: `registerTree(globalThis, 4)` + window prototype chain, executed once at
     `v8_runtime.rs:~1425` (where the second `NATIVE_TO_STRING_SHIM` eval is today). Delete the
     `NATIVE_MASK_SHIM` eval and file.
  4. Worker realms: the same shim program is already spliced last into `worker_exposed_shim`
     (`dom.rs:696`). Add the single sweep at the end of worker binding setup
     (`crates/js/src/worker_shims.rs` path that evals the worker program) so workers get identical
     coverage. Service/shared workers included — verify in `shared_worker` too.
  5. Anything created after step 3 (frame-bridge closures, lazily created shim functions) calls
     `_lumen_native_register` at creation time. Do **not** pre-emptively sprinkle registration calls
     in every shim: the probe in 4.3 decides where they are actually needed.
- `@@toStringTag` sub-step: the registry does not carry prototype tags. Run the probe (4.3) with
  `Object.prototype.toString.call(x)` assertions over platform singletons/prototypes; fix missing
  `Symbol.toStringTag` in the owning shims. This is a separate checklist item, not a registry feature.

### 4.2 Why not per-shim registration everywhere

The sweep is deterministic at the single end-of-install point and already covers everything
reachable; the whack-a-mole came from two mechanisms with different cutoffs, not from the sweep
itself. Explicit registration is needed only for functions created **after** the sweep. The probe
ledger (4.3) is the enforcement mechanism: a new shim that leaks shows up there, so coverage stops
depending on memory. If the probe shows a whole class the sweep cannot reach, extend the sweep —
not 30 shim files.

### 4.3 Steps

1. Inventory: run the probe (below) on current `main` to capture the baseline (BUG-1829 says 71 of
   80 probed APIs leak on the page realm; workers are untested).
2. Implement `native_registry_shim.js`; delete `native_mask_shim.js`; update `dom.rs` constants,
   splice lists and the splice-order test; update `v8_runtime.rs` eval points (exactly one final
   sweep); keep `_lumen_tostring_mark` working or migrate `frame_bridge.rs` in the same commit.
3. Adapt `dom/tests/v8_native_mask.rs` and `dom/tests/v8_native_tostring.rs`; add:
   - getter/setter text (`function get x() { [native code] }`);
   - `Function.prototype.toString` itself reports native;
   - a page-created function still reports real source (no false masking);
   - a late function registered through the hook reports native;
   - worker realm: the same assertions inside a dedicated worker and a shared worker;
   - the registry global is not enumerable and survives the BUG-378 seal.
4. Build the committed probe `scripts/probe_native_api.py` (spec in Appendix B): runs the same
   reachability walk inside Lumen and CfT 157, emits JSON, writes a ledger
   `docs/perf/runs/<date>-native-api.json`. Re-derive the old `.tmp/stall/probe_native.py` logic —
   it is not in the repo (`/.tmp/` is gitignored) and must not be recreated in `.tmp`.
5. Drive the ledger to **zero** non-native platform functions in both realms. Every remaining entry
   is either fixed by an explicit `_lumen_native_register` at creation or by extending the sweep;
   no entries may be waived silently — if a function is intentionally page-visible source, record
   why in the ledger.
6. Live check: `lumen.exe --maximized https://claude.ai` (BUG-1826 repro) with fresh profile —
   the challenge must still complete; run it before and after to isolate regressions.

### 4.4 Verification

| Check | How | Pass criterion |
|---|---|---|
| Unit | `cargo test -p lumen-js dom::tests` (+ targeted modules) | Green; new tests above all present |
| Splice order | `web_api_shim_splices_its_parts_in_source_order` | Updated and green |
| Ledger | `python scripts/probe_native_api.py` | 0 leaks, page + dedicated + shared worker |
| Chrome parity | Same probe against CfT 157 on the same machine | Same native/page classification for the probe's API list |
| Live challenge | `claude.ai` repro | Challenge completes; no engine stall |
| Perf | Time the sweep on `install_dom` (log or `tracy`) | Within noise of the current two mechanisms; if materially slower, report before merging |

**Acceptance A:** one mechanism in the tree (no `native_mask_shim.js`), ledger zero, worker parity,
all tests green, artifacts committed.

---

## 5. Workstream B — one Chrome 157 fingerprint manifest

### 5.1 Design

One data module that every Chrome-bound layer consumes; non-Chrome profiles are untouched.

- New `crates/network/src/http/chrome_manifest.rs`:
  ```rust
  pub struct ChromeManifest {
      pub major: &'static str,                       // must equal lumen_core::CHROME_MAJOR
      pub header_order: &'static [&'static str],     // Chrome 157 order incl. Sec-Fetch-*
      pub h2: H2Spec,                                // header_table_size, enable_push, streams,
                                                     // initial_window, max_frame, header_list,
                                                     // connection_window_increment
      pub tls: TlsSpec,                              // cipher_list (OpenSSL names), sigalgs,
                                                     // groups, alpn
      pub accept_encoding: &'static [&'static str],  // ["gzip", "deflate", "br", "zstd"], in order
  }
  pub const CHROME_157: ChromeManifest = …;
  ```
- UA string and Client Hints stay canonical in `lumen_core::identity` (the crate layering forbids
  core → network). The manifest does **not** duplicate them; a test asserts the manifest agrees with
  `CHROME_MAJOR` / `CHROME_USER_AGENT` / `low_entropy_client_hints`.
- Consumers rewired to the manifest:
  - `h2_settings.rs` Chrome/Strict arm reads `CHROME_157.h2`;
  - `boring.rs` constants become `CHROME_157.tls.*` (same values — this refactor must not change the
    wire);
  - decoders: add `lumen_network::default_content_decoders() -> Vec<Arc<dyn ContentDecoder>>`
    returning the manifest order; replace the 7 registration sites from §2.2 so every client sends
    the same `Accept-Encoding`;
  - `tls/fingerprint.rs` "Chrome 130" reference constants: retarget/annotate to 157 or clearly mark
    as a historical snapshot; do not leave two "current" sources.
- Header order/values already live in `headers.rs`; extract the Chrome arm's ordered table into the
  manifest where feasible. If extraction would churn the builder, keep the builder and add a
  consistency test comparing the produced block to `CHROME_157.header_order` — decide by diff size,
  state the choice in the commit.

### 5.2 Steps

1. Snapshot the current wire before refactoring: capture a request header set with
   `scripts/header_echo_server.py` + `lumen.exe --dump-source` and store it under
   `docs/perf/header-sets/`; capture `tls.peet.ws/api/all` JSON for Lumen and CfT 157 (5.3).
2. Introduce the manifest with the exact current values; wire consumers one at a time; after each
   step, `cargo test -p lumen-network --lib http` and `crates/network/tests/cases/http_client_fingerprint_profile.rs`.
3. Centralize decoders; update all 7 sites; delete now-unused imports.
4. Drift guards:
   - unit: `chrome_manifest_matches_identity` (major/UA/CH brands), `chrome_manifest_encodings_match_clients`
     (each production client builder's `accept_encoding_header()` equals the manifest join),
     `h2_settings_match_manifest`;
   - script `scripts/check_fingerprint_literals.py` (new, CI-runnable): fails if `"157"` or a copied
     Chrome UA appears outside `crates/core/src/identity.rs` + the manifest + tests; run it in CI
     with the doc-link checks.
5. Regenerate the header-set artifact and diff against step 1: the refactor must be byte-identical
   on the wire. Any diff is a bug in the refactor (unless C flipped the identity in the same
   baseline — take the post-C snapshot for B).

### 5.3 Live self-check (the "it really works" gate)

- Script `scripts/fingerprint_selfcheck.py` (new):
  - fetch `https://tls.peet.ws/api/all` with (a) Lumen default and (b) CfT 157, same machine,
    fresh profiles;
  - persist raw JSON to `docs/perf/runs/<date>-peet-lumen.json` / `…-peet-chrome.json`;
  - assert/compare: `http2.sent_frames` settings equal (values and relative order),
    `http2.akamai_fingerprint` equal or documented-diff, header order equal, `tls.ja4`/`ja3_hash`
    recorded (BoringSSL should be close; record exact deltas rather than gating on hashes that
    session state can perturb).
  - fallback when peet is unreachable from the route: `tls.browserleaks.com/json` for JA3/JA4 and
    the local echo server for header order; state the substitution in the run record.
- Run it before B (baseline), after B (regression), and after C if C ships first (identity token is
  visible in UA/CH, not in TLS).
- Not a PR gate: external hosts are flaky. Document it in `docs/automation.md` next to the
  identity/captcha tooling; link the latest run record from the journal.

**Acceptance B:** one manifest, all consumers read it, tests above green, wires byte-identical to
the pre-refactor snapshot, live self-check artifacts committed, `docs/perf/journal.md` entry.

---

## 6. Consolidated verification matrix

| Layer | Tool | Evidence location | Gate? |
|---|---|---|---|
| JS unit + realm parity | `cargo test -p lumen-js …` | test binaries | yes |
| JS ledger (page/worker/CfT) | `scripts/probe_native_api.py` | `docs/perf/runs/<date>-native-api.json` | yes (zero leaks) |
| Wire headers | `scripts/header_echo_server.py`, header-set JSON | `docs/perf/header-sets/` | yes (diff review) |
| TLS/H2 live | `scripts/fingerprint_selfcheck.py` | `docs/perf/runs/<date>-peet-*.json` | pre/post comparison |
| Identity A/B | `scripts/captcha_stand.py` | `docs/perf/runs/`, `docs/perf/journal.md` | read before ship |
| Live challenge | `lumen.exe --maximized claude.ai` | screenshot/log in the bug or journal | yes |
| Network tests | `cargo test -p lumen-network --lib http`, `tests/cases/http_client_fingerprint_profile.rs` | CI | yes |
| Drift guard | `scripts/check_fingerprint_literals.py` | CI log | yes |

## 7. Risks and rollback

- **JS registry:** interactions with the BUG-378 freeze; sweep cost at install time; page code that
  intentionally monkey-patches `Function.prototype.toString` must still work (facade falls back to
  the captured original). Rollback: revert the workstream branch; no data migration.
- **Manifest refactor:** any value typo changes the wire silently — mitigated by the byte-identical
  header-set diff and the peet before/after. Rollback: revert branch; no persisted state.
- **Identity flip:** bot-score regression on sites outside the 2026-10-15 corpus; mitigated by the
  whole-corpus B threshold and the per-site record mechanism. Instant rollback via knob.
- **External checks flakiness:** never gate a PR on a live host; store artifacts and state the route.

## 8. Out of scope (propose separately)

- Pure `Lumen/x.y` UA profile (own `HttpProfile`, H2 settings, rustls ClientHello): opt-in
  transparency mode only; needs its own ADR amendment and corpus run.
- `chrome_brand_google`/`firefox_win` per-site records beyond what the pack already supports.
- Removing rustls for h3/DoT; any TLS stack replacement.
- BUG-1829 structural divergences (SAB exposure, WebGL/Crypto prototypes, RTCPeerConnection) — the
  registry probe ledger will list them, but fixing them is its own task.

## Appendix A — reading list

- `crates/js/src/shim/native_tostring_shim.js`, `shim/native_mask_shim.js`
- `crates/js/src/dom.rs` (shim assembly, `:610-700`), `crates/js/src/v8_runtime.rs` (`:1131`
  `install_v8!`, `:1390-1455` install tail, BUG-378 seal)
- `crates/js/src/dom/tests/{mod.rs,v8_native_mask.rs,v8_native_tostring.rs}`
- `crates/js/src/frame_bridge.rs` (`_lumen_tostring_mark` usage, `:3173+`)
- `crates/network/src/http/{headers.rs,h2_settings.rs,client_hints.rs,mod.rs}`
- `crates/network/src/tls/{boring.rs,fingerprint.rs}`
- `crates/network/src/lib.rs` (`accept_encoding_header`, `:4153`; h2/h3 request paths)
- `crates/shell/src/{page_source.rs,resource_base.rs,download.rs,update.rs}`
- `crates/driver/src/{session.rs,winit_session.rs}`
- `crates/update-manifest/src/compat_pack.rs` (+ `tests.rs`), `assets/compat_pack.json`
- `docs/perf/runs/2026-10-15-ab-matrix.md`, `docs/perf/journal.md`, `docs/perf/header-sets/`
- `scripts/{captcha_stand.py,header_echo_server.py,compat_ab_chrome.py,compat_ab_lumen.py}`

## Appendix B — probe specification (`scripts/probe_native_api.py`)

- Inputs: `--browser lumen|chrome`, `--exe PATH`, `--out PATH`, optional `--env K=V`.
- Page under test: a local `data:`/`file:` probe page (no network), executed in the browser realm.
  For Lumen use the BiDi/MCP live session path the existing scripts use; for CfT 157 use CDP
  (`compat_ab_chrome.py` is the template).
- Probe algorithm inside the page: the same reachability walk as the registry (data props, getters on
  the window chain, prototypes, statics; depth 4; budget 1e6), collect every reachable function,
  classify `native = Function.prototype.toString.call(fn).includes('[native code]')`, and also record
  `Object.prototype.toString.call(obj)` for singletons/prototypes to audit `@@toStringTag`.
- Output JSON: `{ "browser", "version", "realm": "page|dedicated|shared", "functions": [{"path",
  "name", "native", "text"}], "tags": [...] }`; the script prints the counts and fails with exit 1
  when `native == false` for any platform function (page functions are excluded by construction —
  the walk runs before any page script).
- Run the same probe in a dedicated worker and a shared worker realm; store all three ledgers.

## Appendix C — commands

```powershell
# unit gates
cargo test -p lumen-core -p lumen-update-manifest
cargo test -p lumen-network --lib http
cargo test -p lumen-shell --bin lumen config::
cargo test -p lumen-js dom::tests
cargo clippy -p lumen-core -p lumen-network -p lumen-shell -p lumen-update-manifest -p lumen-js --all-targets -- -D warnings

# live artifacts
python scripts/captcha_stand.py --arm lumen --route <label> --label default
python scripts/captcha_stand.py --arm lumen --route <label> --label tokenless --env LUMEN_IDENTITY=chrome_unbranded
python scripts/probe_native_api.py --browser lumen --out docs/perf/runs/<date>-native-api-lumen.json
python scripts/probe_native_api.py --browser chrome --out docs/perf/runs/<date>-native-api-chrome.json
python scripts/fingerprint_selfcheck.py --out-dir docs/perf/runs --date <date>
& "C:\Program Files\Git\bin\bash.exe" scripts/scoped-test.sh
```

**Definition of done (all workstreams):** acceptance tables satisfied; artifacts and journal entries
committed; docs synced in the same commits; three branches merged separately; `main` green after each
push (CI is not awaited — watch it), and no live-host check silently skipped without a recorded
reason.
