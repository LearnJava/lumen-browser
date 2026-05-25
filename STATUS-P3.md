In progress: target-fragment  branch: p3-target-fragment
Next step: set doc.set_target() from URL fragment in parse_and_layout + handle fragment-only link clicks  crates/shell/src/main.rs:1005

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Next:

Queue (Wave 3+):

Recent: web-storage 2026-05-25, navigation-history-api 2026-05-25, preload-scanner-integration 2026-05-25, streaming-feed-bytes 2026-05-25, websocket-js 2026-05-25, http-cache 2026-05-25, link-click-navigation 2026-05-25, fetch-api-runtime 2026-05-22, bfcache 2026-05-22, ime-events 2026-05-22
