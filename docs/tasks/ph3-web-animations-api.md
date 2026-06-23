# Ph3 — Web Animations API runtime

**Developer:** P1 + P2 + P4 · **Branch:** `p1-ph3-web-animations` · **Size:** L · **Crates:** `lumen-js`, `lumen-layout`, `lumen-paint`, `lumen-shell`

> Roadmap source: `docs/plan/phases.md:127` — "Web Animations API runtime [P1+P2+P4]".
> Phase 3 (v1.0) item. This is a **future** task — do not start until Phase 2 closes and a developer claims it in `STATUS-PN.md`.

---

## Status

**Phase 3 (future).** Not started. The JS object model already exists (see Current state); the Phase 3 work is to make WAAPI-driven animations of `transform`/`opacity` ride the same Rust compositor-offload path that CSS `@keyframes`/`transition` already use, instead of round-tripping through `element.style` mutations every RAF tick.

---

## Goal

Promote `element.animate()` / `Animation` / `KeyframeEffect` / `AnimationTimeline` from a self-contained pure-JS shim into a first-class engine runtime:

1. WAAPI animations of compositor-friendly properties (`transform`, `opacity`) are applied as **display-list patches without relayout** — same path as CSS animations (`CompositorAnimFrame` → `build_display_list_with_anim`), instead of per-tick `element.style[prop] = ...` writes that force style recompute + relayout.
2. WAAPI animation timing is scheduled by the **engine animation clock** (one timeline, vsync-driven) rather than each `Animation` instance owning its own `requestAnimationFrame` loop.
3. `document.getAnimations()` / `element.getAnimations()` see **both** CSS-originated animations (from `AnimationScheduler`) and script-created ones — currently they only see the JS registry.
4. Value interpolation reuses the typed Rust interpolators (`AnimationInterpolator` / `LinearInterpolator`) for the offloaded properties instead of the string-lerp helpers in the JS shim.

Non-goals: per-property `composite: add|accumulate` for non-compositor properties (stays JS string-lerp); pseudo-element animation targets; `ScrollTimeline`/`ViewTimeline` *constructed from JS* (CSS scroll-driven timelines already resolve in the scheduler — JS construction is a stretch).

---

## Current state

### WAAPI object model — already implemented (pure JS, decoupled from the engine)

`crates/js/src/dom.rs:11062` — `// Web Animations API Level 1 (W3C Web Animations §3)` block, ~460 lines. A complete, self-contained JS implementation:

- `crates/js/src/dom.rs:11096` — `_wa_doc_timeline` singleton `DocumentTimeline`; `currentTime` getter at `:11090`.
- `crates/js/src/dom.rs:11100` — `_wa_normalize_keyframes` (array + property-indexed forms).
- `crates/js/src/dom.rs:11144` — `_wa_ease` (linear/ease/ease-in/out/in-out/steps/cubic-bezier via Newton).
- `crates/js/src/dom.rs:11171`–`11249` — string interpolators: `_wa_lerp_color`, `_wa_lerp_scalar`, `_wa_lerp_transform`, `_wa_interp_prop`. **String-based, JS-side** — this is the duplicate of the Rust interpolators (P1 target).
- `crates/js/src/dom.rs:11252` — `_wa_compute_at_p(effect, p)` → property map.
- `crates/js/src/dom.rs:11276` — `_wa_iter_progress(timing, ct)` (delay/iterations/direction/fill).
- `crates/js/src/dom.rs:11302` — `KeyframeEffect` ctor + `getTiming`/`updateTiming`/`getKeyframes`/`setKeyframes`.
- `crates/js/src/dom.rs:11327` — `Animation` ctor; props `currentTime`/`startTime`/`playbackRate`/`playState`/`pending` (`:11346`–`11383`).
- Playback control: `play` `:11385`, `pause` `:11399`, `cancel` `:11407`, `finish` `:11418`, `reverse` `:11431`, `updatePlaybackRate` `:11436`.
- **Per-instance RAF loop:** `_scheduleRaf` `:11440`, `_tick` `:11456` — each `Animation` drives itself via `requestAnimationFrame`.
- **Apply path (the hot spot to replace for compositor props):** `_applyAtP` `:11482` writes `eff.target.style[prop] = ...`; `_clearStyles` `:11492`.
- Factory + queries: `_wa_element_animate` `:11507`, `_wa_get_animations_for` `:11515`, `_wa_doc_get_animations` `:11522`.
- DOM surface wiring: `element.animate` / `element.getAnimations` `crates/js/src/dom.rs:4808`; `document.getAnimations` `:5345`; globals `window.Animation` / `KeyframeEffect` / `AnimationPlaybackEvent` `:9249`–`9252`.

### CSS animation + interpolation infra — already implemented (Rust)

`crates/engine/layout/src/animation.rs` — typed interpolation + scheduling that WAAPI should reuse:

- `AnimationInterpolator` trait `:231`, `interpolate()` `:237`; `LinearInterpolator` `:276`.
- Typed interpolators: `interpolate_length` `:318`, `interpolate_color` `:332`, `interpolate_filter_list` `:366`, `interpolate_gradient_stops` `:479`, `interpolate_transform_list` `:516` (with `interpolate_decomposed` `:729` for matrix decomposition).
- `AnimValue` enum `:195`; `KeyframeStyle` `:148` + `parse_keyframe_style` `:157`.
- `AnimationFrame` `:49` (per-node `AnimatedStyle` overrides), `merge`/`merge_from` `:61`/`:80`.
- `to_compositor_frame()` `:97` → `CompositorAnimFrame` `:130` (only `opacity` + `transform`, no relayout); `CompositorOverride` `:120`.
- Rust-side `AnimationScheduler` `:760` (`sync` `:776`, `tick` `:807`) and `TransitionScheduler` `:1130` for CSS transitions.

### Animation scheduler (shell) — the engine clock

`crates/shell/src/animation_scheduler.rs:116` — `AnimationScheduler` (CSS Animations L1 timeline). `tick()` `:133` walks the layout tree, finds `@keyframes` by name, computes interpolated styles → `AnimationFrame`. Drives only **CSS** `animation_names` (`process_node` `:175`, gated on `style.animation_names.is_empty()` `:184`). Scroll-driven progress via `ScrollCtx::progress_for` `:79`. **Knows nothing about JS-created WAAPI animations.**

### Compositor offload path — the P2 target, already wired for CSS

- `crates/shell/src/main.rs:9946` — scheduler tick result stored: `self.anim_frame = ...`.
- `crates/shell/src/main.rs:10186` — `frame.to_compositor_frame()` → patched into the display list (transform/opacity without relayout). Comment at `:10184`: color/background-color stay in `anim_frame` for the future (need relayout).
- Display-list primitives the offload patches: `PushOpacity` `crates/engine/paint/src/display_list.rs:539`, `PushTransform` `:644`; `CompositorAnimFrame` / `CompositorOverride` consumed by `build_display_list_with_anim` (see `crates/engine/paint/src/display_list.rs:10`,`:19`).

### The gap (what Phase 3 fixes)

CSS animations: scheduler → `AnimationFrame` → `CompositorAnimFrame` → display-list patch, **no relayout, vsync-clocked, typed interpolation**.
WAAPI today: each `Animation` → own RAF → `element.style[prop]=...` → **full style recompute + relayout every tick, string interpolation, separate clock**. The two never meet, and `getAnimations()` returns disjoint sets.

---

## Architecture

```
JS:  element.animate(kf, opts)
        → KeyframeEffect (target, normalized keyframes, timing)
        → Animation (effect, timeline) + playback control
        → registered in _wa_animations  AND  bridged to the Rust runtime
                                              │
Bridge (new, lumen-js ↔ shell):              ▼
   Per active WAAPI Animation targeting a real element with only
   compositor props (transform/opacity): emit a typed descriptor
   { node, keyframes(AnimValue), timing } to the shell instead of
   ticking element.style.
                                              │
P4  timeline scheduling ──────────────────────┤
   AnimationScheduler also advances WAAPI animations on the engine
   clock: resolve currentTime from the shared timeline, compute
   iteration progress (delay/iterations/direction/fill), apply
   playbackRate / play / pause / reverse / finish state.
                                              │
P1  value interpolation ──────────────────────┤
   For each active WAAPI animation, interpolate at time t with the
   typed Rust interpolators (interpolate_transform_list / scalar /
   color) → AnimatedStyle, merged into the same AnimationFrame as
   CSS animations.
                                              │
P2  compositor offload ───────────────────────▼
   AnimationFrame.to_compositor_frame() already yields transform +
   opacity overrides. Ensure WAAPI-originated overrides flow through
   build_display_list_with_anim (PushOpacity/PushTransform) with NO
   relayout — same as CSS. Non-compositor props fall back to the JS
   element.style path.
```

**Playback control** (`play`/`pause`/`reverse`/`finish`/`cancel`) stays authored in JS (`Animation.prototype.*`) but mutates a state object the bridge reads; the engine clock (not per-instance RAF) advances `currentTime`. `finished`/`ready` Promises and `onfinish`/`oncancel` events keep firing from JS when the engine reports a state transition.

**`getAnimations()`** must union the JS registry (`_wa_animations`) with CSS-originated animations surfaced from the Rust `AnimationScheduler`. Decide a representation: simplest is to have the CSS scheduler expose lightweight read-only `Animation`-like JS objects (id, playState, currentTime) so `document.getAnimations()` returns both. (Proposed.)

---

## Team split (P1 / P2 / P4)

| Dev | Owns | Files |
|---|---|---|
| **P1** | **Value interpolation at time t.** Reuse typed interpolators for WAAPI keyframes; produce `AnimatedStyle` per node for the bridged animations; merge into the shared `AnimationFrame`. Map JS keyframe property strings → `AnimValue` (`transform`, `opacity` first). | `crates/engine/layout/src/animation.rs` (extend `AnimValue` mapping / a `waapi`-keyframe interpolation entry), bridge consumer in `crates/shell/` |
| **P2** | **Compositor offload.** Make WAAPI transform/opacity overrides ride `to_compositor_frame()` → `build_display_list_with_anim` with no relayout; confirm `PushOpacity`/`PushTransform` patching covers script-created nodes; fall back to `element.style` only for non-compositor props. | `crates/engine/paint/src/display_list.rs`, `crates/shell/src/main.rs:10186` area |
| **P4** | **Animation timeline scheduling.** Advance WAAPI animations on the engine clock inside the shell `AnimationScheduler` (`crates/shell/src/animation_scheduler.rs`): resolve `currentTime` from the shared timeline, compute iteration progress, honor `playbackRate`/`play`/`pause`/`reverse`/`finish`. Retire the per-instance JS RAF loop for bridged animations. | `crates/shell/src/animation_scheduler.rs`, `crates/js/src/dom.rs:11440`–`11480` (remove/gate per-instance RAF) |

Interface-first: P4 publishes a `WaapiAnimState` descriptor type (node, normalized keyframes, timing, playState, currentTime, playbackRate) with `todo!()` stubs; P1 implements interpolation against it; P2 implements the offload sink. The JS bridge that emits descriptors is shared (whoever lands first stubs it).

---

## Entry points (real file:line; *(proposed)* = to be added)

- `crates/js/src/dom.rs:11507` `_wa_element_animate` — factory; *(proposed)* register bridged descriptor when target is a real element and props are compositor-only.
- `crates/js/src/dom.rs:11456` `_wa_tick` / `:11482` `_applyAtP` — *(proposed)* gate: skip `element.style` writes for properties handled by the compositor bridge; keep them only for fallback props.
- `crates/js/src/dom.rs:11440` `_scheduleRaf` — *(proposed)* disabled for bridged animations (engine clock drives them instead).
- `crates/js/src/dom.rs:11522` `_wa_doc_get_animations` / `:11515` `_wa_get_animations_for` — *(proposed)* union with CSS-originated animations from the scheduler.
- `crates/shell/src/animation_scheduler.rs:133` `tick` — *(proposed)* additional pass advancing bridged WAAPI animations and writing their `AnimatedStyle` into the same `AnimationFrame`.
- `crates/shell/src/animation_scheduler.rs:116` `struct AnimationScheduler` — *(proposed)* hold the set of active WAAPI descriptors (mirrored from JS each tick or pushed on `animate()`/`play()`/`cancel()`).
- `crates/engine/layout/src/animation.rs:97` `to_compositor_frame` — reuse as-is (already opacity+transform only).
- `crates/engine/layout/src/animation.rs:516` `interpolate_transform_list` / `:318` `interpolate_length` — P1 reuses for typed WAAPI interpolation.
- `crates/engine/layout/src/animation.rs:195` `enum AnimValue` — *(proposed)* ensure it covers all bridged WAAPI keyframe property kinds.
- `crates/shell/src/main.rs:10186` `frame.to_compositor_frame()` patch site — P2 verifies WAAPI overrides flow here.
- *(proposed)* a `WaapiAnimState` descriptor type (location TBD: `lumen-layout::animation` or a shell-local module) — P4 publishes it.

---

## Steps

1. **P4 — interface.** Add `WaapiAnimState` descriptor type with `todo!()` stubs; add a slot in `AnimationScheduler` to hold active descriptors. Commit as the interface anchor.
2. **JS bridge — emit descriptors.** On `_wa_element_animate` / `Animation.play()` for compositor-only animations, push a descriptor to the Rust side (via the existing JS↔shell channel; reuse the mechanism that already carries `requestAnimationFrame`/event traffic). On `cancel`/`finish`/`pause`, update the state.
3. **P4 — engine-clock scheduling.** In `animation_scheduler.rs:tick`, advance each WAAPI descriptor: compute `currentTime` from the shared timeline + `playbackRate`, derive iteration progress (reuse the `apply_direction` / timing logic already used for CSS at `:196`–`:237`). Stop the per-instance JS RAF (`dom.rs:11440`) for bridged animations.
4. **P1 — typed interpolation.** For each active descriptor at progress t, interpolate keyframes with `interpolate_transform_list` / scalar / color → `AnimatedStyle`; merge into the same `AnimationFrame` the CSS pass builds (`merge_from` `animation.rs:80`).
5. **P2 — offload.** Confirm `to_compositor_frame()` carries WAAPI overrides and `build_display_list_with_anim` patches `PushOpacity`/`PushTransform` for those nodes with no relayout. Gate the JS `_applyAtP` so compositor props are NOT also written to `element.style` (avoid double-apply / relayout).
6. **State transitions back to JS.** When the engine clock reports a bridged animation finished, notify JS so `onfinish` + `finished` Promise fire (`Animation.prototype._onFinish` `dom.rs:11501`).
7. **`getAnimations()` union.** Surface CSS-originated animations so `document.getAnimations()` / `element.getAnimations()` return both sets.
8. **Fallback path intact.** Non-compositor properties (e.g. `width`, `color` requiring relayout) keep the existing JS `element.style` path. Verify no regression for those.

---

## Tests

- **JS unit (lumen-js):** `element.animate({opacity:[0,1]}, 1000)` returns an `Animation` with `playState==='running'`; `currentTime` advances; `pause()`/`play()`/`reverse()`/`finish()`/`cancel()` transition `playState` correctly; `finished` Promise resolves on completion. (Most of this exists logically in the shim — keep it green after the engine takes over the clock.)
- **Bridge / scheduler (shell):** a page with a script-driven `transform`/`opacity` animation produces a non-empty `AnimationFrame` from `AnimationScheduler::tick` and a non-empty `CompositorAnimFrame`; the offloaded node is patched in the display list without a relayout (assert layout box geometry unchanged across ticks).
- **Interpolation parity (layout):** typed interpolation of a WAAPI `rotate`/`opacity` keyframe pair at t=0.5 matches `interpolate_transform_list` / scalar results (regression guard against drift between JS string-lerp and Rust typed-lerp).
- **getAnimations union:** a page with both a CSS `@keyframes` animation and a `element.animate()` — `document.getAnimations().length === 2`.
- **Graphic test:** add a WAAPI demo (script-driven `transform`/`opacity`) under `graphic_tests/` and to `1000000-final.html`, plus a `COVERAGE.md` row, in the implementing commit (per CLAUDE.md rule). Since it is JS-driven, validate via the `--ipc` pipeline (engine clock must tick under `--ipc-server`); use a CPU snapshot baseline if gdigrab timing is flaky (mark as `KNOWN_DEBTORS` only with an OPEN BUG-NNN if it can't reach 0.5%).

## Definition of done

- `element.animate()` of `transform`/`opacity` runs on the engine vsync clock (no per-instance JS RAF for bridged animations) and is applied via `PushOpacity`/`PushTransform` display-list patches **without relayout**.
- Playback control (`play`/`pause`/`reverse`/`finish`/`cancel`), `currentTime`/`startTime`/`playbackRate`, `onfinish`/`finished` all work with engine-driven timing.
- Non-compositor animated properties still work via the JS `element.style` fallback (no regression).
- `document.getAnimations()` and `element.getAnimations()` return both CSS-originated and script-created animations.
- Typed Rust interpolators are the single source of truth for offloaded-property interpolation (no divergence from CSS animation rendering).
- `cargo clippy -p lumen-js -p lumen-layout -p lumen-paint -p lumen-shell --all-targets -- -D warnings` clean; crate tests pass; graphic/IPC test added; `CAPABILITIES.md`, `subsystems/*.md`, `docs/plan/phases.md:127`, and `STATUS-PN.md` updated in the merge commit.
