In progress: timers-async (setTimeout/setInterval/clearTimeout/clearInterval + scheduler.postTask)  branch: p3-timers-async
Next step: реализовать JS timer queue + _lumen_tick_timers + WaitUntil в about_to_wait  crates/js/src/dom.rs:1462

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Next:

Queue (Wave 3+):

Recent: web-apis (URL/URLSearchParams/performance/queueMicrotask) 2026-05-25, persistent-js-runtime 2026-05-25, target-fragment 2026-05-25, web-storage 2026-05-25, navigation-history-api 2026-05-25, preload-scanner-integration 2026-05-25, streaming-feed-bytes 2026-05-25, websocket-js 2026-05-25, http-cache 2026-05-25, link-click-navigation 2026-05-25
