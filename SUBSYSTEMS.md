# Lumen Subsystem State

Per-crate scope, implemented features, deferred items, test counts, and behavioral invariants.
Updated with every plan-item commit. For ground truth — `git log --oneline` + `cargo doc -p <crate>` + read `src/lib.rs`.

## Crates

| Crate | Status | File |
|---|---|---|
| lumen-core | ✅ ext traits + Punycode + URL + hash | [subsystems/core.md](subsystems/core.md) |
| lumen-dom | ✅ arena DOM + InputType + DocumentMode | [subsystems/dom.md](subsystems/dom.md) |
| lumen-html-parser | ✅ FSM tokenizer + tree builder | [subsystems/html-parser.md](subsystems/html-parser.md) |
| lumen-css-parser | ✅ selectors + all Phase 0 properties | [subsystems/css-parser.md](subsystems/css-parser.md) |
| lumen-layout | 🟡 block/inline/flex/grid layout | [subsystems/layout.md](subsystems/layout.md) |
| lumen-paint | 🟡 display list + wgpu renderer | [subsystems/paint.md](subsystems/paint.md) |
| lumen-font | 🟡 TTF + variable fonts runtime | [subsystems/font.md](subsystems/font.md) |
| lumen-encoding | 🟡 detector + CyrillicDecoders + UTF-16 | [subsystems/encoding.md](subsystems/encoding.md) |
| lumen-image | 🟡 PNG + JPEG baseline/progressive | [subsystems/image.md](subsystems/image.md) |
| lumen-js | ✅ QuickJS runtime via rquickjs 0.11 | [subsystems/js.md](subsystems/js.md) |
| lumen-storage | ✅ SQLite + cookies + history + … | [subsystems/storage.md](subsystems/storage.md) |
| lumen-knowledge | 🟡 FTS5 history + notes + read-later | [subsystems/knowledge.md](subsystems/knowledge.md) |
| lumen-network | ✅ HTTP/1.1 + HTTPS + CORS + auth | [subsystems/network.md](subsystems/network.md) |
| lumen-driver | ✅ BrowserSession trait + InProcessSession headless | [subsystems/driver.md](subsystems/driver.md) |
| lumen-shell | 🟡 window + render + event loop | [subsystems/shell.md](subsystems/shell.md) |
| lumen-devtools | ✅ WebSocket + CDP минимум | [subsystems/devtools.md](subsystems/devtools.md) |
| lumen-bench | ✅ pipeline benchmark | [subsystems/bench.md](subsystems/bench.md) |
| lumen-canvas | ✅ Canvas 2D CPU rasterizer (Phase 0) | [subsystems/canvas.md](subsystems/canvas.md) |
| Infrastructure | workspace + test counts + dep policy | [subsystems/infra.md](subsystems/infra.md) |
