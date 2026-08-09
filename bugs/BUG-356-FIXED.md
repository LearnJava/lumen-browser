# BUG-356 — `<a>`/`<link>` do not reflect `href` at all, and the URL-decomposition IDL attributes are missing on every element

**Статус:** FIXED 2026-08-09
**Компонент:** js (`crates/js/src/dom.rs:4554-4571` — the generated bare HTML\*Element interfaces, incl. `HTMLAnchorElement`; `crates/js/src/dom.rs:5499` `_lumen_make_element` — the live element object literal that decides which properties an element wrapper gets)
**Найден:** P2, WPT-VENDOR-encoding (2026-07-27), `run_report.py --all --root encoding --recursive`

## Симптом

Two layers of the same gap, both confirmed outside WPT with `--dump-layout`
probe pages:

1. **`href` is not reflected on `<a>`/`<link>` in either direction.** Reading
   `href` off a parsed live anchor yields `undefined`; assigning `a.href = …`
   creates an ordinary JS own property and never touches the `href` content
   attribute.

   ```
   PROBE created: getAttribute(href)=null | .href=https://example.com/p?q=1#h
   PROBE parsed:  .href=undefined | getAttribute=/p.html?q=1#h
   PROBE img.src=/x.png            ← reflected (special-cased)
   PROBE link.href=undefined
   PROBE a.href=undefined
   ```

   `img.src` *is* wired up, so this is a per-element omission, not a blanket
   "no reflected attributes" design. (Side note in the same probe: `img.src`
   returns the raw attribute `/x.png` rather than the resolved absolute URL the
   spec requires — a smaller, separate wrinkle, not filed on its own.)

2. **None of the URL-decomposition IDL attributes exist on any element.**
   `protocol`, `host`, `hostname`, `port`, `pathname`, `search`, `hash`,
   `origin`, `username`, `password` (HTML LS §4.6.3 "API for a and area
   elements", the `HTMLHyperlinkElementUtils` mixin) are all `undefined`, on
   both created and parsed elements, and on `<area>` too:

   ```
   PROBE parsed: search=undefined pathname=undefined protocol=undefined
                 host=undefined hash=undefined origin=undefined
   PROBE proto has search? false
   PROBE area search=undefined
   ```

`Object.keys()` on a live `<a>` wrapper confirms it gets only the generic
element surface (`tagName`/`id`/`className`/`classList`/`style`/`textContent`/
`innerHTML`/`getAttribute`/… ) — nothing anchor-specific.

## Причина

`HTMLAnchorElement` (like `HTMLLinkElement`, `HTMLAreaElement` isn't even in the
table) is minted by the generic loop at `crates/js/src/dom.rs:4554-4571`, which
builds *bare, non-constructible* interface objects whose only job is to make
`instanceof HTMLAnchorElement` and `'HTMLAnchorElement' in window` resolve
(introduced by BUG-322). Their prototypes are empty `Object.create(
HTMLElement.prototype)` — no accessors are ever installed. The tag→interface
table at `dom.rs:4586` maps `'A': HTMLAnchorElement`, so the plumbing to hang
these properties on already exists; nothing hangs them.

The URL-parsing machinery also already exists and is reusable: `_lumen_parse_url`
(`dom.rs:7355-7396`) returns exactly `{href, protocol, hostname, host, port,
pathname, search, hash, origin}`, and the `URL` class (`dom.rs:10750-10775`)
already exposes those very fields through `prop(...)` accessors. So this is a
wiring omission, not missing capability.

## Масштаб

In the `encoding` category run: **451 subtest failures across 5 test files**,
all with the identical error `Cannot read properties of undefined (reading
'substr')` — the WPT encoder tests build `a.href = "https://example.com/?" +
input` and then read `a.search.substr(1)` to observe how the document encoding
serialized the query string (`encoding/resources/encode-href-common.js`, and the
same idiom inlined in `big5-encoder.html`):

| Test file | Failing subtests |
|---|---|
| `legacy-mb-schinese/gb18030/gb18030-encoder.html` | 254 |
| `legacy-mb-tchinese/big5/big5-enc-ascii.html` | 123 |
| `legacy-mb-schinese/gbk/gbk-encoder.html` | 49 |
| `big5-encoder.html` | 13 |
| `iso-2022-jp-encoder.html` | 12 |

That is the single largest *executed* failure cause of the whole category (the
larger TIMEOUT block is a survey gap, not an engine defect — see
`docs/wpt-status.md`).

Well beyond WPT, and arguably worse there than here: `link.href` /
`anchor.href` is one of the most common DOM reads in real page and framework
code (link rewriting, analytics click handlers, router interception, "is this
link external" checks via `a.hostname !== location.hostname`, tab/anchor
components). Every one of those currently reads `undefined`. Element-side URL
decomposition is also the classic hand-rolled URL parser (`var a =
document.createElement('a'); a.href = url; a.pathname`), still widespread in
older libraries.

## Возможный фикс (не реализован в этой сессии)

- Install a `HTMLHyperlinkElementUtils`-style accessor set on
  `HTMLAnchorElement.prototype` and `HTMLAreaElement.prototype` (add `'AREA'` to
  the tag table at `dom.rs:4586`; `HTMLAreaElement` also needs adding to the
  interface list at `dom.rs:4554`), backed by `_lumen_parse_url` over the
  element's `href` content attribute resolved against the document base URL —
  the same `_lumen_resolve_url`/base-href path `dom.rs:10724` already uses.
- `href` itself: getter → resolved absolute URL, setter → `setAttribute('href',
  v)`. Each decomposition setter re-serializes and writes `href` back.
  `HTMLLinkElement` needs the `href` half only (no decomposition mixin per spec).
- Watch out that these must read through to the live attribute on every get
  (`getAttribute('href')`), not cache at wrapper-construction time — element
  wrappers are interned per nid (`_lumen_element_wrappers`) and outlive
  attribute mutations.
- BUG-346 (`Url::resolve()` doesn't collapse `..`) sits on the same resolution
  path; fixing that first would keep the new getters from inheriting a known
  wrong answer.

Not fixed in this session — P2-wpt vendors and surveys, code fixes are P3's lane
(`CLAUDE.md` developer assignments).

## Фикс (P3, 2026-08-09)

Part 1 (`href` reflection) turned out to already be closed as a side effect of
[BUG-383](BUG-383-FIXED.md) (2026-07-29, the IDL-reflection unification): `href`
is installed as a `url`-kind entry on `HTMLAnchorElement.prototype`/
`HTMLAreaElement.prototype`/`HTMLLinkElement.prototype` (`dom.rs`, the
`_lumen_install_reflection` calls right before the block below), which already
reads through `_lumen_reflect_url` on every get and writes the raw attribute on
every set — the `undefined`/dead-setter symptom from the original probe no
longer reproduces. Only part 2, the `HTMLHyperlinkElementUtils` decomposition
mixin, was still missing; that is the part fixed here.

Added right after the `<a>`/`<area>` reflection tables in `dom.rs`:

- `_lumen_hyperlink_url_get(self)` — resolves the live `href` attribute via the
  existing `_lumen_reflect_url` (document-base-URL resolution, already used by
  the `href` getter itself) and parses it with `_lumen_parse_url`; returns
  `null` when there is no `href` (an absent/empty attribute), so every getter
  below can fall back to `''` — matches the spec's "if this's url is null" case.
- `_lumen_hyperlink_url_set(self, mutate)` — the inverse: decomposes the
  current `href`, lets `mutate` edit the parsed parts, re-serializes
  (`protocol + '//' + host + pathname + search + hash`) and writes the result
  back via `_lumen_set_attr(n, 'href', …)`. No-op when there is no `href` yet,
  per spec (setting `a.protocol` on an anchor with no `href` does nothing).
- `_lumen_install_hyperlink_utils(proto)` installs `protocol`/`hostname`/
  `host`/`port`/`pathname`/`search`/`hash` as paired get/set accessors built on
  the two functions above, plus a read-only `origin`, plus inert `username`/
  `password` stubs (get `''`, set no-op) — same fidelity as `URL.prototype`,
  which doesn't model credentials either. Called once each for
  `HTMLAnchorElement.prototype` and `HTMLAreaElement.prototype`.
  `HTMLLinkElement` is deliberately not touched — it reflects `href` but is not
  part of the `HTMLHyperlinkElementUtils` mixin.

All accessors read/write through the live attribute on every call (no caching
at wrapper-construction time), same requirement called out in the "possible
fix" section above — element wrappers are interned per nid
(`_lumen_element_wrappers`) and outlive attribute mutations. BUG-346 (dot-segment
collapsing) was already fixed by the time this landed, so the new getters don't
inherit that wrong answer.

Verified outside WPT with a `--dump-layout` probe (getters, setters, the
`<area>` mixin, `<link>` correctly *not* getting it, and the exact WPT idiom
from this bug's original report — `a.href = base + '?' + input; a.search.substr(1)`).
Added 8 regression tests in `dom::tests::v8_nav_url_storage`
(`anchor_href_reflects_content_attribute`, `anchor_url_decomposition_getters`,
`anchor_search_substr_matches_wpt_encoder_idiom`,
`anchor_without_href_decomposition_is_empty`,
`anchor_decomposition_setters_rewrite_href`,
`anchor_host_setter_updates_hostname_and_port`,
`area_href_and_decomposition`, `link_does_not_get_url_decomposition_mixin`).
`cargo test -p lumen-js --features v8-backend --lib` 2514/2514 green (2506
pre-existing + 8 new). `cargo clippy -p lumen-js -- -D warnings` still fails on
15 pre-existing dead-code errors unrelated to this change (`offscreen_canvas.rs`,
`worker.rs`) — confirmed identical on `main` before this fix, not introduced
here.
