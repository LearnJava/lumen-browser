# STATUS-P1 — Feature Development

**Developer:** Программист 1 (Feature development — any subsystem from roadmap)

---

## In progress

**9F.3 — Tor circuit: `--tor` CLI mode**  
Branch: p1-ph2-9f3-tor  
Next step: `extract_tor_mode()` в `shell/src/main.rs` + применить TorBrowser-профиль через config + connectivity check  
Files: `crates/shell/src/main.rs:184`, `crates/shell/src/config.rs:204`

---

## Next

### PH3 — Phase 3: v1.0 «Full Browser»

| # | Задача | Размер | Крейты |
|---|--------|--------|--------|
| PH3-1 | **DevTools Elements styled-rules panel** — список CSS-правил, применённых к выбранному элементу; поверх existing inspector + `InProcessSession::computed_style_json`; правая панель inspector.rs | M | `lumen-shell` (devtools/) |
| PH3-2 | **`lumen-bidi-server` standalone крейт** — вынос BiDi из `shell/src/bidi/` в отдельный крейт; shell импортирует; PH2-11 задел | M | `lumen-shell`, новый `lumen-bidi-server` |
| PH3-3 | **getUserMedia Phase 1** — реальный захват аудио через WinMM/WASAPI (Windows) / ALSA (Linux); `getUserMedia({audio:true})` → `MediaStream` с PCM | L | `lumen-js`, `lumen-shell` |
| PH3-4 | **Offscreen Canvas Phase 1** — `new OffscreenCanvas(w,h)` + `transferControlToOffscreen()` + `postMessage` transfer; CPU render path | M | `lumen-js`, `lumen-paint` |
| PH3-5 | **Web Workers Phase 1** — `new Worker(url)` → отдельный QuickJS-контекст в треде; `postMessage` / `onmessage` channel; `importScripts()` | L | `lumen-js`, `lumen-shell` |

---

## Recent merges

| Дата | Задача | Описание |
|------|--------|---------|
| 2026-06-15 | PH2-7: Accessibility tree + platform bridges Phase 1 | `WinUiaBridge` Phase 1: `init_hwnd()` + `NotifyWinEvent` (EVENT_OBJECT_FOCUS/REORDER/STATECHANGE) + `handle_wm_get_object` + `ax_role_to_msaa()` (60 вариантов). 125 тестов в lumen-a11y. |
| 2026-06-15 | PH2-3: Профили + шифрование | `profile_vault` — AES-256-GCM key wrapping, PBKDF2-HMAC-SHA256 (100k iter). `ProfileRegistry`: `set_password`, `clear_password`, `unlock`, `is_encrypted`. 11 unit-тестов. |
| 2026-06-15 | PH2-2: Site isolation Phase 1 | `lumen-network::coop` — COOP/COEP/CORP парсинг; 27 тестов. `window.crossOriginIsolated` + pipeline wiring. |
| 2026-06-15 | PH1-8: Preload scanner | `PreloadScanner` struct поверх `PushTokenizer`; инкрементальный scan. 35 тестов. |
| 2026-06-15 | PH1-7: Compositor thread + Property Trees | `InProcessCompositor` + `ThreadedCompositor` + `PropertyTrees::build()` + `scroll_page_by`. 15 тестов. |
| 2026-06-15 | PH1-6: Stacking contexts + CSS Painting Order | `build_display_list_ordered` подключён к driver; 3 теста на CSS 2.1 Appendix E. |
| 2026-06-15 | PH1-5: CI/CD для Linux/macOS/Windows | `.github/workflows/ci.yml` + `release.yml`; 4 бинарных пакета. |
| 2026-06-15 | PH1-4: Network service в отдельном процессе | `lumen-ipc` крейт; `RemoteNetworkTransport`; `--network-service` флаг. |
| 2026-06-15 | PH1-15: T1 (paused) | `pause_event_loop()`/`unpause_event_loop()` в `PersistentJs`; 6 тестов. |
| 2026-06-15 | PH1-2: Progressive / streaming rendering pipeline | 60 Hz throttle; `LoadEvent::CssLoaded`; параллельная загрузка CSS; 3 теста. |
| 2026-06-15 | PH1-9: lumen-mcp-server крейт | 5 ресурсов + 7 инструментов; StdioTransport + TcpTransport; shell `--mcp` / `--mcp-port N`. 15 тестов. |
| 2026-06-14 | PH1-10..14: Auto-wait / Per-context isolation / A11y first-class / TabState / Image LRU | Все подтверждены в коде; STATUS обновлён. |
| 2026-06-14 | PH2-1/4/5/6/8/9/10/11/12/15/16: Phase 2 features | HTTP/2, anti-fingerprinting, meta viewport, Shadow DOM runtime, IME, mix-blend-mode stacking, BiDi, GPU LRU, Glyph LRU — все подтверждены. |
| 2026-06-14 | Y-series (Y-2..Y-5): Web Platform Phase 4 | unicode-range в lumen-font, scrollbar-width/color, color-scheme, scroll snap events — все реализованы. |
