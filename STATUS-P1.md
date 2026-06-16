# STATUS-P1 — Feature Development

**Developer:** Программист 1 (Feature development — any subsystem from roadmap)

---

## In progress

idle

---

## Next

### PH3 — Phase 3: v1.0 «Full Browser»

| # | Задача | Размер | Крейты |
|---|--------|--------|--------|
| PH3-1 | ~~**DevTools Elements styled-rules panel**~~ ✅ завершена | M | `lumen-shell` (devtools/) |
| PH3-3 | ~~**getUserMedia Phase 1**~~ ✅ завершена | L | `lumen-js`, `lumen-shell` |
| PH3-4 | ~~**Offscreen Canvas Phase 1**~~ ✅ завершена | M | `lumen-js`, `lumen-paint` |
| PH3-5 | ~~**Web Workers Phase 1**~~ ✅ завершена | L | `lumen-js` |
| PH3-9 | ~~**HTML5 Drag and Drop API**~~ ✅ завершена | M | `lumen-dom`, `lumen-js`, `lumen-shell` |
| PH3-11 | ~~**`<audio>` element Phase 1 — HTMLAudioElement real playback**~~ ✅ завершена | L | `lumen-core`, `lumen-js`, `lumen-shell` |
| PH3-12 | ~~**`<video>` element Phase 1 — HTMLVideoElement GIF playback**~~ ✅ завершена | M | `lumen-js`, `lumen-paint`, `lumen-shell` |
| PH3-13 | ~~**Screen Wake Lock API Phase 1**~~ ✅ завершена | M | `lumen-core`, `lumen-js`, `lumen-shell` |

---

## Recent merges

| Дата | Задача | Описание |
|------|--------|---------|
| 2026-06-16 | PH3-14: File Input API Phase 1 | `register_file_token()` + thread-local `FILE_REGISTRY`; нативные биндинги `__lumen_file_read_text`/`__lumen_file_read_base64`; `File.prototype.text()`/`arrayBuffer()`/`stream()` читают реальные байты через токены; `entries_to_json_with_tokens()` в shell; JS не видит сырых путей файловой системы. 18 новых тестов lumen-js + 4 lumen-shell. |
| 2026-06-16 | PH3-13: Screen Wake Lock API Phase 1 | `WakeLockProvider` трейт + `NullWakeLockProvider` в lumen-core::ext; `set_wake_lock_provider()` + `__lumen_wake_lock_request`/`__lumen_wake_lock_release` биндинги + обновлённый JS-шим в lumen-js; `PlatformWakeLock` (`SetThreadExecutionState` на Windows, no-op на Linux/macOS) в shell/src/platform/wake_lock.rs. 23 новых теста. |
| 2026-06-16 | PH3-12: `<video>` element Phase 1 | `VideoGifStore` + `VideoPlaybackState` в lumen-js (без зависимости от lumen_image); 12 нативных биндингов `__lumen_video_*` + JS-шим; `BoxKind::Video` → `DrawImage { src: "video:{nid}" }` в display_list; `tick_video_gifs()` в shell — декодирует GIF, регистрирует кадры, продвигает анимацию. |
| 2026-06-16 | PH3-11: `<audio>` element Phase 1 | `AudioPlaybackProvider` трейт в lumen-core; 16 нативных биндингов `__lumen_audio_*` + JS-шим (play/pause/seek/timeupdate/ended/loop/canPlayType) в lumen-js; `PlatformAudioPlayer` на rodio 0.19 с per-handle audio thread + mpsc в lumen-shell. 39 новых тестов. |
| 2026-06-16 | PH3-10: Pointer Events API Level 3 | `pointer_captures` HashMap в lumen-dom; `pointer_capture.rs` Rust-биндинги + `pointer_capture_nid` Arc в lumen-js; `Element.setPointerCapture/releasePointerCapture/hasPointerCapture` + `gotpointercapture`/`lostpointercapture`; L3 свойства (altitudeAngle, getCoalescedEvents); shell routing + implicit release на pointerup. 10 новых тестов, итого 2091 lumen-js. |
| 2026-06-16 | PH3-9: HTML5 Drag and Drop API | `is_element_draggable()` в lumen-dom (HTML LS §9.3.3); `DndState` + `DND_THRESHOLD` + `js_drag_event()` в shell; полный lifecycle: dragstart→drag/dragover/dragenter/dragleave→drop/dragend. 231 тест lumen-dom, 2081 lumen-js. |
| 2026-06-16 | PH3-8: Web Animations API Level 1 (JS runtime) | `DocumentTimeline`, `KeyframeEffect`, `Animation` (play/pause/cancel/finish/reverse), `AnimationPlaybackEvent`; `element.animate()` + `element.getAnimations()`; `document.timeline` + `document.getAnimations()`; интерполяция (числа/цвета/transform), easing (linear/ease/cubic-bezier/steps), fill/direction/iterations. 21 тест. |
| 2026-06-16 | PH3-7: `contentEditable` + Input Events Level 2 + Selection routing | `node_is_contenteditable()`, `find_editing_host()` в lumen-dom; 5 Rust-биндингов + JS-свойства (`contentEditable`, `isContentEditable`) + `_lumen_handle_contenteditable_key()` в lumen-js; маршрутизация клавиш в shell через DOM (не eval_js). 17 новых тестов. |
| 2026-06-16 | PH3-6: `<dialog>` focus management + `<form method="dialog">` | `showModal()` фокусирует [autofocus]-потомок или сам диалог; `close()` восстанавливает предыдущий фокус; `<form method="dialog">` закрывает родительский `<dialog>`. `find_ancestor_dialog()` в lumen-dom. 8 новых тестов. |
| 2026-06-15 | PH3-5: Web Workers Phase 1 | `importScripts()` для data: и blob:lumen/ URL; `WorkerBlobStore` (Arc-shared); `atob`/`btoa` в worker globals; WORKER_SHIM оборачивает createObjectURL для auto-регистрации blob'ов. 20 новых тестов, итого 47 worker-тестов. |
| 2026-06-15 | PH3-4: Offscreen Canvas Phase 1 | `create_offscreen_from_pixels()` + `transferControlToOffscreen()` + `postMessage(data,[transfer])` с сериализацией OffscreenCanvas через сентинели. 8 новых тестов. |
| 2026-06-15 | PH3-3: getUserMedia Phase 1 | `AudioCaptureProvider` + `PlatformAudioCapture` (cpal/WASAPI/ALSA); `__lumen_start_audio_capture` + JS MediaStreamTrack. 247 тестов. |
| 2026-06-15 | PH3-2: `lumen-bidi-server` standalone крейт | WebDriver BiDi сервер вынесен из `shell/src/bidi/` в отдельный крейт. `lumen_bidi_server::spawn` — единственный публичный API. 89 тестов. |
| 2026-06-15 | PH3-1: DevTools Styles-таб | `ComplexSelector::to_css_str()`, `matched_rules_for_node()`, `InspectorTab::Styles` — CSS правила для выбранного узла в DevTools. 16 новых тестов. |
| 2026-06-15 | 9F.3: Tor circuit (`--tor` CLI) | `extract_tor_mode()` + `check_tor_connectivity()` + override `FingerprintProfile` → TorBrowser + `socks5://127.0.0.1:9050` + `no_persistent_state`. Завершает ADR-007 (все 6 слоёв). 6 тестов. |
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
