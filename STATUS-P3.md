In progress: —
Next step: —

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Next:
- fix-bug028-resize: BUG-028 — relayout-on-resize + maximized window; shell/src/main.rs Resized handler запускает BUG-027-паттерн (width=0 frame)
- shadow-dom-js: P3-часть Shadow DOM — JS биндинги: Element.attachShadow(), customElements.define() + lifecycle callbacks (connectedCallback/disconnectedCallback/attributeChangedCallback); P1-часть (FlatTree + layout wiring) уже готова
- rendering-steps-order: правильный порядок rendering steps (style → layout → paint как cascade) в shell event loop; сейчас шаги частично перемешаны; PerformanceObserver timing
- sop-enforcement: применить SOP-классификатор в shell для postMessage / storage / cookie-jar — Origin-проверки при cross-origin обращениях; lumen-network::Origin готов, CORS preflight готов
- http-tls-client: HTTP/1.1 + TLS через rustls — загрузка реальных URL (не только file://); provisional dep rustls + tokio; интегрировать в HttpClient + shell navigation

Queue (Wave 2):
- mixed-content-enforcement: применить lumen-network::classify_subresource_request в HttpClient — блокировать blockable mixed-content до TCP connect; classifier готов, enforcement нет
- sandbox-dom-apply: применить SandboxFlags из <iframe sandbox> в shell при навигации iframe — ограничить JS/forms/popups; SandboxFlags::parse_sandbox_value готов
- fts-omnibox: интегрировать lumen-knowledge::HistoryFts с omnibox — @history prefix + Porter stemmer для русского языка
- performance-observer: PerformanceObserver JS API — PerformanceEntry (measure/mark/resource), performance.now() high-res, wiring в shell rendering step timing
- http2-client: HTTP/2 через h2 crate (provisional) — multiplexing для реальных сайтов; бэкэнд-замена HttpClient::connect без смены публичного API
- preconnect-hints: обработать <link rel=preconnect> из preload_scanner — открыть TCP+TLS соединение заранее; уменьшит latency первого ресурса

Queue (Wave 3+):
- service-workers: Service Worker API (fetch intercept + cache API + background sync); Phase 2
- push-api: Web Push + Notifications API (VAPID, push subscription); Phase 2
- profiles-system: multi-profile — отдельные хранилища cookies/history/storage per profile; Phase 2
- ime-input: IME ввод для CJK/русского через OS compositor API (winit CompositionEvent); Phase 2
- devtools-protocol: Chrome DevTools Protocol (CDP) subset — Elements + Console + Network; Phase 2
- crash-dump: panic hook → сохранить последние 50 EventSink-событий + стектрейс в crash.log; Phase 2

Recent: no-scrollbar-flag (--no-scrollbar CLI флаг для screenshot-пайплайна) 2026-05-26, observers-api (MutationObserver + ResizeObserver + IntersectionObserver + getBoundingClientRect) 2026-05-26, raf-js (requestAnimationFrame / cancelAnimationFrame) 2026-05-25, dom-dirty-relayout (layout invalidation after JS DOM mutations) 2026-05-25, timers-async (setTimeout/setInterval/scheduler.postTask) 2026-05-25, web-apis (URL/URLSearchParams/performance/queueMicrotask) 2026-05-25, persistent-js-runtime 2026-05-25, target-fragment 2026-05-25, web-storage 2026-05-25, navigation-history-api 2026-05-25, preload-scanner-integration 2026-05-25, streaming-feed-bytes 2026-05-25, websocket-js 2026-05-25, http-cache 2026-05-25
