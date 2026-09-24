
// LIB-11 (BUG-693): `_lumen_parse_url` used to be the ~55-line hand-rolled
// string-splitter above (removed by this change — see BUGS-FIXED.md) — it
// now delegates to the native `_lumen_url_parse` binding onto
// `lumen_core::url::Url` (the `url` crate, WHATWG URL Standard state
// machine, ADR-027; see `crates/js/src/js_url.rs`). Every call site in this
// codebase expects the same nine-field-plus-`hasAuthority` object shape
// (`href`/`protocol`/`username`/`password`/`hostname`/`host`/`port`/
// `pathname`/`search`/`hash`/`origin`/`hasAuthority`) whether the parse
// succeeds or fails, so a failed native parse (returns `null`) still yields
// that shape with every field empty and `protocol: ''` — the same "invalid
// URL" signal callers already checked for (`if (!p.protocol) throw ...`,
// `URL.canParse`'s try/catch around the constructor).
//
// `_LUMEN_PAGE_URL` injected by Rust before this shim runs.
function _lumen_parse_url(url) {
    var href = String(url || '');
    var p = _lumen_url_parse(href, undefined);
    if (p) return p;
    return { href: href, protocol: '', username: '', password: '',
             hostname: '', host: '', port: '',
             pathname: '/', search: '', hash: '', origin: '',
             hasAuthority: false };
}
