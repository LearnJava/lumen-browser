# BUG-693 — The JS-visible URL machinery (`_lumen_parse_url`/`_url_resolve`, `URL`/`location`/`HTMLHyperlinkElementUtils`) is a hand-rolled string-splitter, not the WHATWG URL Standard state machine

**Статус:** OPEN
**Компонент:** js (`crates/js/src/dom.rs:4633` — `_lumen_parse_url`; `dom.rs:8074` — `_url_resolve`; both feed `URL`/`location`/`URLSearchParams.href`-linkage and, via `_lumen_reflect_url`/`_lumen_hyperlink_url_get`/`_lumen_install_hyperlink_utils`, every `<a>`/`<area>` IDL accessor)
**Найден:** P2, WPT-VENDOR-url, 2026-08-09

## Симптом

Категория `url` (`tests/wpt/url/`, 50 файлов, пин совпадает с прочим бэклогом) —
вендорена и прогнана целиком (`run_report.py --all --root url --recursive`,
~2:19, 44 отобранных id) — **39/44 harness OK, 1742/7451 сабтестов**. Дешёвый
профиль по предикторам (0 `.https.`, 0 `testdriver.js`, 2 variant-хита)
оказался ложным — категория выполнилась почти целиком и дала огромный объём
живого сигнала, крупнейший провальный кластер во всём бэклоге на сегодня.

Провалы сконцентрированы в файлах, гоняющих `urltestdata.json`
(WHATWG's собственный эталонный набор ~10k кейсов URL-парсинга) через три
разных JS-поверхности:

- `url-constructor.any.html` (все варианты) — **223/899** — `new URL(input, base)`
- `a-element.html`/`a-element-origin.html` (все варианты) — **5/1313** —
  `<a href>`/`.protocol`/…/`.origin` (тот же датасет, тот же движок парсинга)
- `url-setters*.any.html`/`.window.html` — **116/1102** — сеттеры `.protocol`/
  `.host`/… на `URL` и на `<a>`/`<area>`
- `url-origin.any.html` — **156/413**
- `toascii.window.html` — **193/784**, `IdnaTestV2.any.html` — **987/2671**
  (хостовая IDNA/punycode-обработка — часть того же парсера)
- `url-statics-canparse.any.html` — **4/8** (`URL.canParse()` возвращает
  `true` на заведомо мусорных парах аргументов)

Примеры (`urltestdata.json`, воспроизводится по любому из перечисленных
файлов):

```
Parsing: <http://example\t.\norg> against <http://example.org/foo/bar>
  href expected "http://example.org/" but got "http://example\t.\norg"
Parsing: <http://user:pass@foo:21/bar;par?b#c> against <...>
  username expected "user" but got ""
Parsing: <https://test:@test> without base
  href expected "https://test@test/" but got "https://test:@test"
Parsing: <http:foo.com> against <http://example.org/foo/bar>
  href expected "http://example.org/foo/foo.com" but got "http:foo.com"
ToASCII("faß.de")   expected "xn--fa-hia.de" but got "faß.de"
Setting <a://example.net>.protocol = 'b'
  expected "b://example.net" but got "a://example.net"
URL.canParse(undefined, undefined)   expected false but got true
```

## Причина

`_lumen_parse_url` (`dom.rs:4633-4670`) is ~35 lines of ad-hoc string
splitting on `://`, `@`, the last `:`, `/`, `?`, `#`. It implements none of
the WHATWG URL Standard's parsing algorithm:

- **No input sanitization** — leading/trailing C0-or-space and embedded
  ASCII tab/newline are never stripped (spec §4.4 steps 1-2), so they leak
  straight into `href`/`hostname` verbatim.
- **No special-scheme table** — `http:`/`https:`/`ws:`/`wss:`/`ftp:`/`file:`
  are not distinguished from arbitrary schemes; a `file:` URL never gets its
  spec-mandated empty-host special-casing, an opaque-path scheme like
  `mailto:`/`javascript:` is only handled by the `else` branch (`cIdx =
  href.indexOf(':')`), which can't tell "opaque path" from "no authority
  present yet, still needs relative resolution" apart.
- **Userinfo is discarded, not parsed** — `authority.slice(atIdx + 1)`
  throws away everything before `@` outright, so `.username`/`.password`
  are unreachable from any URL that has them (`user:pass@foo` → `username:
  ""`).
- **No IDNA/punycode/ToASCII** anywhere in this function — every hostname
  is stored as the literal input string. `IdnaTestV2.any.html`'s 1684
  failures and `toascii.window.html`'s 591 are this gap surfacing through
  two different test harnesses over the same missing step.
- **No percent-encoding of path/query/fragment components** per the
  relevant WHATWG percent-encode sets.
- **`_url_resolve` (`dom.rs:8074-8099`) treats `/^[a-zA-Z][a-zA-Z0-9+.-]*:/`
  as "already absolute" and returns the string verbatim**, without parsing.
  This misses the spec's core "scheme relative" behavior: when the input's
  scheme textually equals the base's scheme *and that scheme is special*,
  the URL parser continues resolving against the base instead of treating
  the input as a fresh absolute URL — the `http:foo.com` example above.
  `_url_resolve`'s own relative-path branch does a manual `..`/`.`
  dot-segment normalizer (RFC 3986 §5.2.4, correct in isolation, added for
  BUG-346) but nothing upstream of it applies IDNA/userinfo/percent-encoding
  either, since it delegates back to the same `_lumen_parse_url`.
- **Setters silently no-op instead of validating** — `url-setters.any.html`'s
  183 failures are systematic: `.protocol = 'b'` on `a://example.net` doesn't
  change anything (no scheme-validation state machine, no re-serialization
  after a successful scheme swap), `.protocol = 'file'` doesn't restructure
  the URL per the file-URL special case, etc. Same root: there is no actual
  URL setter algorithm, just field reassignment on the return value of
  `_lumen_parse_url` (see `_lumen_hyperlink_url_set`, `dom.rs:10969`, and the
  analogous `URL.prototype` setters, already flagged from a different angle
  by [BUG-375](BUG-375-FIXED.md) — that bug is about *most* `URL.prototype`
  setters being literal no-op stubs; this one is that even the one that
  *is* wired (`protocol`, `hostname`, …) does not implement the spec's
  validation/normalization steps).
- **`URL.canParse()`** (used by `url-statics-canparse.any.html`) delegates to
  the same non-validating parser, so it returns `true` for `undefined`,
  malformed authorities, and other inputs the spec requires to fail parsing.

This function is entirely separate from the real, RFC 3986/WHATWG-adjacent
`Url` type used for actual navigation (`crates/network`, the one that got
its dot-segment fix under [BUG-346](BUG-346-OPEN.md)) — the JS-visible
`URL`/`location`/`<a>` surface has its own, much cruder reimplementation
that never shares logic with the network layer's parser.

## Масштаб

Not a narrow edge case — this is the parsing/serialization core for every
JS-visible URL object (`URL`, `location`, `<a>`/`<area>` via
`HTMLHyperlinkElementUtils`, and indirectly `URLSearchParams`'s
`.href`-linkage). `urltestdata.json` alone accounts for well over 2500 of
this run's ~5700 subtest failures across the three surfaces it drives
(`url-constructor`, `a-element*`, `url-setters*`); `IdnaTestV2`/`toascii`
account for another ~2200. Any WPT category that constructs a `URL` from a
non-trivial input (IDNA hostnames, credentials, unusual schemes, embedded
whitespace) will keep re-surfacing this same root cause under a different
file name — treat further reconfirmations as expected, not new findings,
unless they exercise a code path outside `_lumen_parse_url`/`_url_resolve`.

## Дальше

Fix scope is large (a real implementation of WHATWG URL Standard §4:
basic URL parser, host parser with IDNA via a punycode crate, URL setter
algorithms per §4.3, serialization). Given the network crate already has a
spec-adjacent `Url` type with dot-segment resolution, the lowest-risk path
is likely exposing *that* type to the JS layer (native binding) instead of
maintaining a second, parallel JS-only parser — but that is an architecture
decision for whoever picks this up, not decided here.
