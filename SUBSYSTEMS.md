# Lumen Subsystem State

Per-crate scope, implemented features, deferred items, test counts, and behavioral invariants.
Updated with every plan-item commit. For ground truth — `git log --oneline` + `cargo doc -p <crate>` + read `src/lib.rs`.

The Status cell below is a one-line summary; **the per-crate file is authoritative** and the code
above both. When a row and its file disagree, the file wins and the row is the thing to fix.

## Crates

| Crate | Status | File |
|---|---|---|
| lumen-a11y | 🟡 AXTree (ARIA roles + names + states) + platform bridges | [subsystems/a11y.md](subsystems/a11y.md) |
| lumen-core | ✅ ext traits + URL (vendored `url`, LIB-6) + Punycode + ICC + hash | [subsystems/core.md](subsystems/core.md) |
| lumen-dom | ✅ arena DOM + InputType + DocumentMode | [subsystems/dom.md](subsystems/dom.md) |
| lumen-html-parser | ✅ all 23 HTML5 insertion modes + Declarative Shadow DOM | [subsystems/html-parser.md](subsystems/html-parser.md) |
| lumen-css-parser | 🟡 complete CSS3 selector set; per-property status in `CSS-SPECS.md` | [subsystems/css-parser.md](subsystems/css-parser.md) |
| lumen-layout | 🟡 block + inline-flow + flex + grid + replaced + cascade | [subsystems/layout.md](subsystems/layout.md) |
| lumen-paint | 🟡 display list + wgpu renderer (femtovg fallback, ADR-017) + deterministic CPU rasterizer | [subsystems/paint.md](subsystems/paint.md) |
| lumen-font | 🟡 TTF/OTF + WOFF2 + variable fonts + COLR/CPAL + rustybuzz shaping (LIB-2/LIB-3) | [subsystems/font.md](subsystems/font.md) |
| lumen-encoding | ✅ detector + decoders + Unicode provider + hyphenation | [subsystems/encoding.md](subsystems/encoding.md) |
| lumen-image | ✅ PNG + JPEG + WebP + GIF + AVIF + SVG (resvg, LIB-4) | [subsystems/image.md](subsystems/image.md) |
| lumen-js | ✅ V8 (`rusty_v8` 150.1.0) — the only engine since S12b (2026-08-04) | [subsystems/js.md](subsystems/js.md) |
| lumen-storage | ✅ SQLite + IndexedDB + cookies + history + profiles + HTTP cache + 15 further stores | [subsystems/storage.md](subsystems/storage.md) |
| lumen-knowledge | ✅ FTS5 over history + notes + read-later + omnibox integration | [subsystems/knowledge.md](subsystems/knowledge.md) |
| lumen-ipc | ✅ TCP IPC channel (PH1-4) | [subsystems/network.md](subsystems/network.md) |
| lumen-bidi-server | 🟡 WebDriver BiDi WebSocket server, thin `AutomationHandle` front-end | [subsystems/driver.md](subsystems/driver.md) |
| lumen-mcp | 🟡 MCP tool-server binary, thin `AutomationHandle` front-end | [subsystems/driver.md](subsystems/driver.md) |
| lumen-network | ✅ HTTP/1.1 + HTTP/2 + HTTP/3 (QUIC) + TLS + WebSocket + SSE + DoH/DoT + cache + CORS/auth + IPC transport | [subsystems/network.md](subsystems/network.md) |
| lumen-driver | ✅ BrowserSession trait + InProcessSession headless | [subsystems/driver.md](subsystems/driver.md) |
| lumen-shell | 🟡 window + render + event loop | [subsystems/shell.md](subsystems/shell.md) |
| lumen-devtools | ✅ WebSocket + minimal CDP | [subsystems/devtools.md](subsystems/devtools.md) |
| lumen-bench | ✅ pipeline benchmark | [subsystems/bench.md](subsystems/bench.md) |
| lumen-canvas | ✅ Canvas 2D CPU rasterizer (`CanvasRenderingContext2D` + Path2D + ImageData) | [subsystems/canvas.md](subsystems/canvas.md) |
| lumen-ai | ⬜ crate skeleton only (feature-flagged, not in default bundle) | [subsystems/ai.md](subsystems/ai.md) |
| lumen-chrome | 🟡 build.rs parse-gate + id/action codegen + runtime host + hit-test/hover/dispatch; the default and only chrome renderer since CC-14/CC-15-6 | [subsystems/chrome.md](subsystems/chrome.md) |
| Infrastructure | workspace + test counts + dep policy | [subsystems/infra.md](subsystems/infra.md) |
