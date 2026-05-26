In progress: observers-api  branch: p3-observers-api
Next step: MutationObserver + ResizeObserver + IntersectionObserver + getBoundingClientRect  crates/js/src/dom.rs

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Next:

Queue (Wave 3+):

Recent: raf-js (requestAnimationFrame / cancelAnimationFrame) 2026-05-25, dom-dirty-relayout (layout invalidation after JS DOM mutations) 2026-05-25, timers-async (setTimeout/setInterval/scheduler.postTask) 2026-05-25, web-apis (URL/URLSearchParams/performance/queueMicrotask) 2026-05-25, persistent-js-runtime 2026-05-25, target-fragment 2026-05-25, web-storage 2026-05-25, navigation-history-api 2026-05-25, preload-scanner-integration 2026-05-25, streaming-feed-bytes 2026-05-25, websocket-js 2026-05-25, http-cache 2026-05-25
