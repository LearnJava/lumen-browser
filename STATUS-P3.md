In progress: —
Next step: —

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Bug fixes rule: P3 does NOT fix bugs. Discovered bugs → add to BUGS.md + P5 picks up.

Next: — (все задачи переданы P1 2026-05-27 → STATUS-P1.md)

Queue: — (см. STATUS-P1.md: Wave 1 строка 22, Wave 2 строки 44-45, Phase 2+ строки 47-100)

Recent: fts-omnibox (OmniboxSuggestion + @history prefix + dropdown + ArrowUp/Down + SearchHistory recording) 2026-05-27, sandbox-dom-apply (IframeInfo + collect_iframes + check_popup_gate + shell-гейты) 2026-05-27, find-in-page-regex (Ctrl+R regex mode + collect_visible_text + TextFragment matching) 2026-05-27, mixed-content-enforcement (classify_subresource_request в HttpClient) 2026-05-27, click-hint-overlay (F + hint-key vimium-style kbd-навигация) 2026-05-27, http-tls-client (BrotliContentDecoder + Ctrl+L адресная строка для URL-навигации) 2026-05-27, sop-enforcement (postMessage targetOrigin check + CookieProvider + document.cookie) 2026-05-27, rendering-steps-order (spec-correct render loop order + PerformanceObserver + paint timing) 2026-05-27, shadow-dom-js (Element.attachShadow, shadowRoot, customElements.define/get/whenDefined, lifecycle callbacks) 2026-05-27, no-scrollbar-flag (--no-scrollbar CLI флаг для screenshot-пайплайна) 2026-05-26, observers-api (MutationObserver + ResizeObserver + IntersectionObserver + getBoundingClientRect) 2026-05-26, raf-js (requestAnimationFrame / cancelAnimationFrame) 2026-05-25, dom-dirty-relayout (layout invalidation after JS DOM mutations) 2026-05-25, timers-async (setTimeout/setInterval/scheduler.postTask) 2026-05-25, web-apis (URL/URLSearchParams/performance/queueMicrotask) 2026-05-25, persistent-js-runtime 2026-05-25, target-fragment 2026-05-25, web-storage 2026-05-25, navigation-history-api 2026-05-25, preload-scanner-integration 2026-05-25, streaming-feed-bytes 2026-05-25, websocket-js 2026-05-25, http-cache 2026-05-25
