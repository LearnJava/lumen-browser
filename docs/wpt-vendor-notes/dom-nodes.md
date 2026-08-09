# WPT vendor notes — `dom/nodes`

## Vendoring (`tests/wpt/VENDOR.md`)

One full test category ("start tiny", per this task's Prerequisites) — includes the S4 smoke test, `Element-hasAttribute.html` (`Document-createElement.html`, floated as an "e.g." example when this file was first drafted, turned out to need un-vendored `/common/dummy.xml`/`dummy.xhtml` iframe fixtures and `async_test` — not actually trivial; picked a genuinely self-contained synchronous test instead).
