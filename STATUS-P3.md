In progress: rendering-steps-order  branch: p3-rendering-steps-order
Next step: fix duplicate transition tick + reorder CSS anim before rAF + PerformanceObserver  main.rs:2907, dom.rs:2841

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P3 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Next:
- rendering-steps-order: правильный порядок rendering steps (style → layout → paint как cascade) в shell event loop; сейчас шаги частично перемешаны; добавить PerformanceObserver timing
- sop-enforcement: применить SOP-классификатор в shell для postMessage / storage / cookie-jar — Origin-проверки при cross-origin обращениях; lumen-network::Origin готов, CORS preflight готов
- http-tls-client: HTTP/1.1 + TLS через rustls — загрузка реальных URL (не только file://); provisional dep rustls + tokio; интегрировать в HttpClient + shell navigation

Queue (Wave 2):
- mixed-content-enforcement: применить lumen-network::classify_subresource_request в HttpClient — блокировать blockable mixed-content до TCP connect; classifier готов, enforcement нет
- sandbox-dom-apply: применить SandboxFlags из <iframe sandbox> в shell при навигации iframe — ограничить JS/forms/popups; SandboxFlags::parse_sandbox_value готов
- fts-omnibox: интегрировать lumen-knowledge::HistoryFts с omnibox — @history prefix + Porter stemmer для русского языка
- http2-client: HTTP/2 через h2 crate (provisional) — multiplexing для реальных сайтов; бэкэнд-замена HttpClient без смены публичного API
- preconnect-hints: обработать <link rel=preconnect> из preload_scanner — открыть TCP+TLS соединение заранее

Queue (Wave 3+):
- service-workers: Service Worker API (fetch intercept + cache API + background sync); Phase 2
- push-api: Web Push + Notifications API (VAPID, push subscription); Phase 2
- profiles-system: multi-profile — отдельные хранилища cookies/history/storage per profile; Phase 2
- ime-input: IME ввод для CJK/русского через OS compositor API (winit CompositionEvent); Phase 2
- devtools-protocol: Chrome DevTools Protocol (CDP) subset — Elements + Console + Network; Phase 2

Recent: shadow-dom-js (Element.attachShadow, shadowRoot, customElements.define/get/whenDefined, lifecycle callbacks) 2026-05-27, no-scrollbar-flag (--no-scrollbar CLI флаг для screenshot-пайплайна) 2026-05-26, observers-api (MutationObserver + ResizeObserver + IntersectionObserver + getBoundingClientRect) 2026-05-26, raf-js (requestAnimationFrame / cancelAnimationFrame) 2026-05-25, dom-dirty-relayout (layout invalidation after JS DOM mutations) 2026-05-25, timers-async (setTimeout/setInterval/scheduler.postTask) 2026-05-25, web-apis (URL/URLSearchParams/performance/queueMicrotask) 2026-05-25, persistent-js-runtime 2026-05-25, target-fragment 2026-05-25, web-storage 2026-05-25, navigation-history-api 2026-05-25, preload-scanner-integration 2026-05-25, streaming-feed-bytes 2026-05-25, websocket-js 2026-05-25, http-cache 2026-05-25
