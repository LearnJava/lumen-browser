# ADR-002: Two-tier dependency policy (permanent + provisional)

## Status

Accepted

## Date

2026-05-15

## Context

Lumen's core principle is "write it ourselves." But some subsystems (GPU API, TLS, JS engine, SQL engine) are universally outsourced even by the best teams. We need a policy that:

1. Preserves the "own engine" identity for HTML/CSS/DOM/layout/paint/font/network/encoding
2. Allows pragmatic third-party use for subsystems where rolling-your-own is universally considered wrong
3. Enables Phase 0→3 speed for areas that would otherwise block progress

## Decision

Two dependency categories:

**Permanent exceptions** (5 total) — never implement ourselves:
1. `winit` — OS windowing / event loop
2. `wgpu` — GPU API
3. `rustls` + `webpki-roots` — TLS
4. `rusqlite` (bundled) — SQLite
5. JS engine (currently `rquickjs`, future `rusty_v8`)

**Provisional accelerators** — use now, can replace later, each has a trait-anchor in `lumen-core::ext` and an explicit graduation criterion:
- `icu4x` (Unicode segmentation/line-break/bidi) — `UnicodeProvider`
- `brotli-decompressor`, `ruzstd` — `ContentDecoder`
- `zune-jpeg` + `zune-png` — `ImageDecoder`
- `idna`, `psl` — `IdnaProvider`, `PublicSuffixList`
- `hyphenation` — `HyphenationProvider`
- others per §5 of lumen-plan.md

**Lumen core (never outsourced):** HTML parser, CSS parser, DOM, style/cascade, layout, paint, font parsing/rasterization, URL, HTTP/1.1+2, DNS, adblock matcher, knowledge layer, UI shell.

## Alternatives considered

| Alternative | Why rejected |
|---|---|
| Binary policy (only 5 exceptions, write everything else) | Blocks Phase 0→3: would require writing our own DEFLATE, JPEG DCT, Unicode segmenter before the browser renders a page. |
| Unrestricted third-party use | Eliminates Lumen's identity; every addition becomes a supply-chain risk; makes graduation criteria irrelevant. |

## Consequences

- **Positive:** fast Phase 0→2 progress; well-known libraries handle edge cases we haven't encountered yet; provisional list is explicit and auditable.
- **Negative:** provisional dependencies can become permanent through inertia; graduation criteria are tracked but require annual review discipline.
- **Future:** after Phase 2 (working browser), run an annual review of provisional list — decide what to replace with own code. Most graduation criteria are "realistically never" (stable format, no architectural value in reimplementing), but the review keeps the policy honest.
