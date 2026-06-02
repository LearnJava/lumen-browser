# ADR-009: Shell UI — Panel/Surface system (retained display list, no external GUI framework)

## Status

Accepted

## Date

2026-05-31

## Context

Lumen needs a browser shell UI: tabs, sidebars, address bar, bookmark popover, command
palette, picture-in-picture windows, privacy dashboard, focus mode, and more. The list
grows with every design iteration and will keep growing.

The core challenge is **extensibility without regression**: adding a new panel, a new
floating window, or a new design variant must not require touching existing panels,
layout code, or the rendering pipeline.

Secondary constraint: **cross-platform by construction**. The solution must run
identically on Windows, macOS, and Linux without `#[cfg(target_os = ...)]` scattered
across panel code.

Tertiary constraint: **performance**. The browser shell must feel instant — 60 fps hover
states, zero-latency popups. A heavyweight GUI framework that rebuilds the full widget
tree every frame is the wrong default.

Existing infrastructure that must be reused:
- `winit` — window creation and OS event handling (already cross-platform)
- `wgpu` — GPU rendering (already cross-platform: DX12/Metal/Vulkan/WebGPU)
- `lumen-paint::DisplayList` — typed draw commands (FillRect, DrawText, etc.)
- `lumen-driver::Renderer` — submits DisplayList to wgpu

The shell already draws some UI via DisplayList (address bar overlay, scrollbar, find
bar, link hints). The pattern works; the question is how to scale it to full browser
chrome without losing maintainability.

## Decision

Build a **Panel/Surface system** on top of the existing DisplayList pipeline. No
external GUI framework (no egui, no iced, no Tauri). Every UI element is a `Panel`
that:

1. Declares **where it lives** via `Surface` (docked in layout tree / floating overlay /
   separate OS window / modal dialog)
2. Declares **how big it is** via `SizeRule`
3. **Paints itself** into a given `Rect` by returning a `DisplayList`
4. **Reacts to events** by returning `EventResponse` (consumed / ignored / Command)
5. Is **registered** into `SurfaceManager` — the single coordinator

`SurfaceManager` owns the layout tree of slots, composites all panel DisplayLists in one
wgpu pass, routes OS events to the correct panel, and executes `Command`s against
`AppState`.

Full specification: [`docs/shell-ui-architecture.md`](../shell-ui-architecture.md).

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| **egui + egui-wgpu** | Immediate mode: rebuilds the full widget tree every frame (60× per second) even when nothing changed. Adds ~1.5 MB to binary. Style customisation is limited — pixel-perfect match to the sand/indigo design requires fighting the framework. Good for quick prototypes, wrong for production browser chrome. |
| **iced** | Retained mode and declarative, better architecture than egui. But the wgpu backend is less battle-tested, the API surface is large, and integration with an existing wgpu render loop is non-trivial. Overkill for a panel system we fully control. |
| **Tauri / webview for chrome** | Would use a system webview to render the browser shell. Defeats Lumen's core principle (ADR-001): we build the engine, we don't wrap one. Also: no control over rendering, no cross-platform consistency, heavy runtime. |
| **Self-hosted: render chrome with Lumen itself** | Ideal long-term architecture (shell described in HTML+CSS, rendered by Lumen's own engine). Not practical in Phase 0–1: requires working interactive elements (onclick, input), JS, and embedded viewport. Planned for Phase 3+; Panel system is designed to be replaced by self-hosted chrome transparently. |
| **Raw wgpu + custom widget toolkit** | What we are doing — but without the Panel abstraction layer. Rejected because without the abstraction every new panel requires touching layout code, composite code, and event dispatch. The Panel trait IS the abstraction that makes raw wgpu manageable. |

## Consequences

- **Positive:**
  - Zero new runtime dependencies — builds on existing winit + wgpu + DisplayList
  - Retained mode: DisplayList rebuilt only when state changes, not every frame
  - Adding a new panel = one new file implementing `Panel` trait; no existing files change
  - Adding a new slot = one new node in the layout tree; no existing panels change
  - Cross-platform by construction: all OS surface calls go through winit/wgpu; panel
    code has zero `#[cfg(target_os)]`
  - Theme is a data structure; switching design variants = swap one `Theme` value
  - Binary stays small: no framework overhead
  - Path to self-hosted chrome: `Panel` trait can be implemented by a Lumen-rendered
    HTML frame in Phase 3; the surface manager does not need to change

- **Negative / trade-offs:**
  - Text editing widgets (address bar, tag editor, search field) must be implemented
    from scratch — no framework-provided text boxes. Mitigated by the fact that
    `lumen-shell::address_bar` already implements the pattern.
  - Accessibility (screen readers, ARIA) requires manual wiring to platform accessibility
    APIs — no framework handles this automatically. Deferred to Phase 2.
  - Animation beyond simple lerp requires implementing an animation scheduler — already
    partially done in `lumen-shell::animation_scheduler`.

- **Future:**
  - Phase 3+: replace `Panel` implementations with Lumen-rendered HTML fragments.
    `SurfaceManager` remains unchanged; panels simply delegate `paint()` to the engine.
  - If winit ever becomes insufficient (e.g., Wayland popups, macOS vibrancy), add a
    thin `PlatformSurface` trait in `lumen-shell::platform` — panel code stays clean.
  - Accessibility APIs: `HitTarget` already carries semantic element type; wire to
    `AccessKit` when ready.
