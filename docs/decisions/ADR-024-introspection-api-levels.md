# ADR-024: introspection API levels — internal / experimental / stable

## Status

Accepted — the two questions that were open at drafting (token escape hatch, and
whether L2 exists before v1.0) were decided by the user on 2026-07-30; see
§Decided questions.

## Date

2026-07-30

## Context

Lumen already has a broad introspection surface, catalogued in
[`docs/automation.md`](../automation.md): per-stage text dumps
(`--dump-source` → `--dump-layout` → `--dump-display-list`), phase profiling
(`LUMEN_PROFILE_TREE`, `LUMEN_FRAME_LOG`), a deterministic mode, typed read-only
resources on `lumen-driver` (`computed_style_snapshot`, `layout_snapshot`,
`query_a11y`), 8 MCP tools + 5 MCP resources, and a WebDriver BiDi MVP.

A design discussion on 2026-07-30 (four rounds, two AI reviewers) concluded that
this layer is the project's most defensible differentiator, but only if two
things are true: the engine becomes *measured* first (see the parallel
`WPT-RUN` track, [`docs/tasks/p2-wpt-runner-throughput.md`](../tasks/p2-wpt-runner-throughput.md)),
and the surface is designed as a contract rather than accreted tool-by-tool.

The forcing constraint is a direct conflict between two properties we want at
the same time:

- **Speed of observation.** The value of owning the engine is that a new
  observation point ("show me the invalidation list") can be added in an evening
  and be scriptable the next day. That requires freedom to change shapes.
- **Dependability.** A published API becomes valuable exactly when external
  consumers can rely on it — i.e. when it stops changing. Chromium's CDP is
  fragmented across many domains not out of carelessness but because of the
  weight of that compatibility promise.

These pull in opposite directions, and the cost of getting it wrong is
asymmetric: an accidentally-published internal structure has to be supported
long after the internals it mirrors are gone.

The timing makes this urgent rather than theoretical. The engine's internals are
mid-rewrite: the V8 cutover is done (ADR-018) but `rquickjs` removal is still in
flight (`P3-v8-s12b`), incremental restyle is half-landed and paused by user
decision (BUG-341, slices S16–S27 merged), and identity inside the layout tree
is currently overloaded in three different ways (ADR-025). Freezing a rich API
on top of that state would consume exactly the freedom we are trying to buy.

## Decision

Three levels, with an explicit promotion rule. A capability's level is a
property of the capability, not of the crate it lives in.

### L0 — Internal

Rust APIs inside `lumen-driver` / `lumen-layout` / `lumen-paint` / `lumen-js`.

- **No stability guarantee.** May change or disappear in any commit, no ADR, no
  deprecation window.
- **Not reachable over any wire protocol** — not MCP, not BiDi, not IPC, not CDP.
- Consumers: Rust tests in this repo, `lumen-driver` snapshot gates, and
  debugging sessions.
- This is the default level. New introspection lands here first unless there is
  a reason for it not to.

### L1 — Experimental (wire-exposed, unstable)

Reachable over MCP/BiDi, but explicitly marked as unstable:

- MCP tool and resource names carry an `x-` prefix (`x-explain-element`,
  `resource://x-display-list`) **and** `"experimental": true` in the
  `tools/list` / `resources/list` response.
- May change shape or vanish between commits. Nothing outside this repository
  may depend on it; inside the repository, a dependency is allowed and its
  breakage is the changing commit's problem to fix in the same commit.
- Consumers: our own Python tooling (`graphic_tests/`, `scripts/`, `tests/wpt/`)
  and agent-driven sessions.

### L2 — Stable

A deliberately small, versioned surface. Admission requires **all four**:

1. The data has a definition that outlives the current implementation — either a
   W3C spec (`getComputedStyle`, `getBoundingClientRect`) or an ADR of ours
   (identity → ADR-025).
2. The wire shape is covered by a test in this repo that fails on shape change,
   not just on value change.
3. It is documented in [`docs/automation.md`](../automation.md) with its level
   stated.
4. Removal or an incompatible change requires a new ADR.

**Promotion L1 → L2** requires: two independent consumers, 30 days with no shape
change, and the four admission criteria above. Promotion is a deliberate act
recorded in `docs/automation.md`; nothing is promoted by inaction.

**Demotion** L2 → L1 is not available. That is the point of L2.

### Access model

Today `--mcp-port` / `--mcp-live-port` / `--bidi-port` / `--ipc-server` bind
`127.0.0.1` with **no authentication** (`crates/mcp/src/main.rs:31`). Any local
process can navigate the browser, read the DOM, take screenshots and execute JS.
For a browser whose stated differentiator is privacy, that must not survive into
a release with a richer introspection surface attached to it.

Decided: loopback-only binding stays, and a token becomes mandatory by default —
auto-generated per run, printed to stderr next to the port line, accepted as an
MCP `initialize` parameter and a BiDi `session.new` capability. Tracked as
`DEVX-15`, and it gates the release, not the track.

### Decided questions (user, 2026-07-30)

- **Q1 — no escape hatch.** There is **no `--mcp-allow-anonymous`** and no other
  way to run the automation ports without a token. Every consumer reads the token
  from stderr, including our own (`graphic_tests/run.py`, `scripts/*_perf.py`,
  `tests/wpt/*`) — those edits are inside `DEVX-15`'s scope, not a follow-up.
  Rationale: every consumer of these ports today is ours, so being strict costs a
  one-time edit in ~5 scripts; a convenience flag would outlive the convenience,
  spreading into instructions and third-party scripts until the token is
  decorative. A future session must not re-introduce the flag "just for CI" —
  reopening this needs a new ADR.

- **Q2 — Option A: no L2 before v1.0.** Every wire-exposed introspection
  capability stays **L1** (`x-` prefixed, `"experimental": true`) for the whole of
  Phase 3. The four L2 admission criteria and the promotion rule above stay in
  force as the mechanism, but nothing is promoted through them yet. Rationale:
  ADR-025 reshapes how elements are identified, so anything geometry-flavoured
  promoted before that lands would be promoted onto a key we already know is
  wrong; and with zero external consumers the promise buys nothing today while
  costing the freedom the whole track depends on. The decision is deliberately
  the reversible one — A → B is available at any time, B → A is not.

  Practical consequence: the de-facto-stable tools (`navigate`, `wait`, `query`,
  `eval`, `screenshot`) **keep their current unprefixed names** rather than being
  renamed to `x-`. They predate this ADR and renaming them would break our own
  tooling for no gain; they are simply not L2 — they are unpromoted L1 with
  legacy names. New capabilities get the `x-` prefix. Revisit at v1.0.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| One flat API, everything scriptable, no levels | This is the accretion path that produced CDP's sprawl. It also freezes internals that are actively being rewritten (`rquickjs` removal, ADR-025 identity reshape) — the cost lands precisely where we have the least stability today. |
| Rust-only introspection, nothing wire-exposed | Kills the whole direction: our own Python tooling (`graphic_tests`, `tests/wpt`, `scripts/perf_*`) already drives the browser over MCP and would have no path to the new data. |
| Mirror CDP's domain layout for familiarity | The stated advantage of owning the engine is one coherent causal view instead of assembling it from six protocol domains. Copying the domain split discards the only thing we are actually better at. |
| Defer the whole question until the first tool ships | The first tool is what sets the precedent. After `explain_element` ships without a level, every subsequent tool inherits "no level" as the norm, and the decision gets made by default rather than on purpose. |
| Version the whole protocol instead of per-capability levels | A single protocol version forces lockstep: one experimental tool would either block a version bump or drag an unstable shape into a stable release. Per-capability levels let the fast and slow parts coexist. |

## Consequences

- **Positive:** internals stay free to churn while `rquickjs` removal, the
  ADR-025 identity reshape and (if resumed) BUG-341 land. New observation points
  cost an evening again, because L0/L1 carry no promise. The `x-` prefix makes
  "do not build on this" visible at the call site instead of buried in docs.
- **Positive:** the access-model decision is made once, here, rather than
  re-litigated per tool.
- **Negative:** two-tier bookkeeping. Every new capability needs a level chosen
  and, on promotion, a rename (`x-explain-element` → `explain-element`) that
  breaks our own callers. That churn is intentional — it is the cost that keeps
  L2 small.
- **Negative:** `docs/automation.md` grows a level column and must be kept
  accurate, or the levels become fiction.
- **Future:** revisit at the v1.0 (Phase 3 → v1.0) boundary. The trigger for
  revisiting Q2 earlier is the first external consumer outside this repository;
  the trigger for revisiting Q1 is nothing short of a new ADR.

## Related

- [ADR-025](ADR-025-identity-propagation.md) — the identity contract that
  everything geometry- or paint-flavoured depends on. Ordered **before** the
  first `DEVX` slice.
- [ADR-018](ADR-018-v8-cutover.md), [ADR-021](ADR-021-css-chrome-engine.md) —
  the flag-strategy idiom (explicit rollback lever) reused by `DEVX-15`.
- [`docs/tasks/p1-introspection-track.md`](../tasks/p1-introspection-track.md) —
  `DEVX-7…16` briefs and dependency order.
- [`docs/automation.md`](../automation.md) — the surface catalogue this ADR
  assigns levels to.
