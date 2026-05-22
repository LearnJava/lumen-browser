In progress: Fetch API runtime (fetch/Request/Response/Headers/AbortController в JS shim)  branch: fetch-api-runtime
Next step: добавить _lumen_fetch нативный биндинг + JS-классы Request/Response/Headers/AbortController  crates/js/src/dom.rs

CSS rule: P3 does NOT implement CSS properties. P4 owns all CSS.
  P3 exposes shell hooks (scroll events, OS APIs, network fetch) only.
  When a new shell hook is needed for a CSS property → add it and
  add a line to STATUS-P4.md "Needs wiring".

Next:

Queue (Wave 3+):

Recent: fetch-api-runtime 2026-05-22, bfcache 2026-05-22, ime-events 2026-05-22, sw-fetch 2026-05-22, js-bindings 2026-05-22, sse-client 2026-05-22, sse-parser 2026-05-21, navigation-api 2026-05-21
