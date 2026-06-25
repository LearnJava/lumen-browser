# ADR-015: Swappable chrome (browser UI) — ChromeView abstraction supporting native + web backends

## Status

Accepted (architecture / direction). The native chrome (ADR-009) remains the current
shipping path; this ADR adds the abstraction that keeps the UI layer swappable and
**deliberately defers** the native-vs-web choice. Not yet implemented — target Phase 3+.

## Date

2026-06-25

## Context

The design exploration that produced many candidate interface concepts (article-first reading,
task-centric tabs, profile/container switching, knowledge/memory, privacy-transparency, future
feature surfaces such as the AI sidebar and knowledge graph) raised three requirements that the
current hand-coded shell does not structurally support:

1. **Implement** many visually and behaviourally distinct interface concepts over the same engine.
2. **Switch between them freely at runtime** — like changing a theme today, but deeper (layout too).
3. **Add new interfaces later** without touching the engine.

Two implementation philosophies surfaced, and there is **no clear winner yet**:

- **A — Native chrome.** The UI is drawn in Rust (today: `lumen-shell` via the femtovg backend,
  on the Panel/Surface system of ADR-009). Distinct interfaces are data-driven **UI profiles**:
  layout (where panels go) + `Palette` tokens (colours/spacing, already centralised) + behaviours.
  Strength: performance, no privileged bridge. Limit: a radically different interface needs code.

- **B — Web chrome (dogfooding).** The browser UI is itself an HTML/CSS/JS bundle **rendered by
  Lumen's own engine**, exactly how Firefox (XUL→HTML) and Chrome (WebUI, `chrome://`) work. A
  distinct interface is a bundle; it talks to the browser through a **privileged JS↔Rust bridge**.
  Strength: the engine dogfoods itself, the design mock-ups become near-real chrome, interfaces/
  themes/new views are just bundles, and plugins can ship UI. Cost: per-frame render overhead and
  a privileged bridge to secure.

We want to **avoid an irreversible early commitment** to A or B and instead decide based on
evidence gathered during development.

Constraints:
- The engine and the browser **model** (tabs, navigation, history) must not be coupled to either
  view. Anything A can do the contract must express, and vice versa.
- **Switching a view must not lose state** — open tabs, scroll, history live in the model, not the view.
- A **new interface must be addable without engine changes**.
- The native path must reuse the existing `Palette` + `panel_layout` (ADR-009), not reinvent them.
- Plugins (ADR-013) must be able to contribute interfaces/skins under the inbound-only capability model.
- This realises the user-facing customization vision of §12.10 (themes as JSON + CSS overrides for
  chrome, rearrangeable UI blocks, plugin-supplied UI blocks).

## Decision

Introduce a **stable boundary** between the browser model/controller and the UI, then keep
**both** view backends behind it — feature-gated and runtime-selectable — mirroring the
swappable-backend idiom already used for rendering (ADR-010, `RenderBackend` / `LUMEN_BACKEND`).

1. **`BrowserController` (the contract).** The single API the UI talks to: tab operations
   (open/close/switch/group/set-aside), navigation (go/back/forward/reload/stop), omnibox
   (query/suggest), panels, the command registry, and read access to history/bookmarks/settings.
   It is the *only* coupling point between the UI and the rest of the browser.

2. **`ChromeView` (the view trait).** Every interface is a `ChromeView` that renders from the
   model and emits controller actions on input. Views are **stateless over the model**:
   ```
   trait ChromeView {
       fn render(&mut self, model: &BrowserModel);              // draw the current state
       fn handle_input(&mut self, ev: InputEvent) -> Vec<ControllerAction>;
       fn mount(&mut self, ctl: &dyn BrowserController);        // + unmount / resize / set_theme
   }
   ```
   (Signatures illustrative — the point is the boundary, not the exact methods.)

3. **State lives only in the model.** Switching a view = re-instantiate a `ChromeView` over the
   same model and re-render. No view persists tab/navigation state.

4. **Two backends behind the trait, selectable like the render backend:**
   - `NativeChrome` — built on ADR-009's Panel/Surface system + `Palette` + `panel_layout`.
     Interfaces = UI profiles (data).
   - `WebChrome` — renders an HTML/CSS/JS bundle through the Lumen engine; exposes
     `BrowserController` to bundle JS as a **privileged, capability-limited** bridge.
   - Selection: `LUMEN_CHROME` env var (`native` | `web`); feature flags
     (`chrome-native` default, `chrome-web` behind a flag, like `webgpu`/`avif` today);
     auto-fallback to `native` if `web` fails to initialise.

5. **The A-vs-B winner is deferred — on purpose.** The asset we commit to is the *contract*, not a
   backend. Outcomes explicitly allowed (see “Future”): pick one and delete the other (engine
   untouched), keep both permanently (native for low-end/max-perf, web for rich customization), or
   go hybrid (native frame + web panels).

6. **Plugins contribute interfaces** through the same contract, under ADR-013's inbound-only
   capability model — a plugin-supplied skin/interface gets no more access than its granted
   capabilities.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Commit to native (A) only now | Fast, but forecloses dogfooding and rich/web-driven customization; reversing later is expensive |
| Commit to web (B) only now | Maximally extensible, but per-frame perf, a11y, and bridge security are unproven; too risky as the *only* path |
| Keep the single hard-coded shell (status quo) | Cannot satisfy “switch freely” or “add new interfaces without engine changes” |
| Adopt an external GUI framework (egui/…) for the chrome | Rejected by ADR-009 (retained display list, no external GUI framework); would not give web-style skinning either |
| Theme-only (colours/tokens), no layout swap | `Palette` already does this; insufficient — interfaces differ in *layout and behaviour*, not just colour |

## Consequences

- **Positive:**
  - The irreversible A-vs-B choice is deferred and made evidence-based.
  - Engine and model are untouched when adding or switching interfaces — the contract is the only seam.
  - B can be dogfooded (engine renders its own UI) while A stays the stable fallback.
  - Runtime interface/theme/layout switching becomes a natural extension of today's `Palette` + `panel_layout`.
  - Plugins (ADR-013) can ship whole interfaces, realising §12.10’s customization vision.
- **Negative / trade-offs:**
  - **Two view layers to maintain** — without discipline one bitrots; CI/review must keep both in sync.
  - The contract risks becoming a **least-common-denominator** unless *capability negotiation* is added
    (a view declares what it supports), otherwise B could do things A cannot and vice versa.
  - B’s **privileged JS bridge is an attack surface** and must be strictly capability-limited (cf. ADR-013).
  - B adds **per-frame render overhead** (the engine paints the chrome every frame); mitigated by engine
    speed and layer caching.
  - Requires **disciplined MVC separation** — no view-specific assumptions may leak into the model;
    enforced at review.
- **Future / decision criteria:** during Phase 3, collect for each backend: per-frame perf + memory
  budget, **cost to add a new interface**, plugin-UI story, a11y/UIA bridge quality, bridge security
  surface (B only), binary-size impact. **Trigger to revisit:** once both backends can render the
  *same* reference interface, run a compare pass (perf + visual parity, analogous to ADR-010’s
  `CompareBackend`), then pick one / keep both / go hybrid. Prior-art reference points for the same
  tension: Firefox (XUL→HTML), Chrome (WebUI), VS Code (web UI for extensibility), Zed (native GPU UI
  for performance).

## Relationships

- **Builds on** [ADR-009](ADR-009-shell-panel-system.md) — the native backend uses its Panel/Surface
  system, `Palette`, and `panel_layout`.
- **Same idiom as** [ADR-010](ADR-010-render-backend-abstraction.md) — swappable backend behind a trait,
  feature-gated + env-selectable + compare mode.
- **Capability model from** [ADR-013](ADR-013-wasm-plugin-sandbox.md) — plugin-supplied interfaces/skins.
- **Realises** §12.10 “Кастомизация UI” in [docs/plan/knowledge.md](../plan/knowledge.md).
