
// GAP-ORIGIN: the `Origin` interface (HTML LS §7.1.1, whatwg/html#11846) —
// `[Exposed=*]`, so this part is spliced into the page shim and into every
// worker scope (`worker_exposed_shim`). The URL-to-origin step and the
// registrable-domain lookup behind `isSameSite()` are the native
// `_lumen_url_origin` (`crates/js/src/origin.rs`).
//
// An origin record is either `{opaque: true, id}` or the tuple
// `{opaque: false, scheme, host, port, site}`. Opaque identity is the `id`:
// every opaque origin minted here gets a fresh one, and every `Origin` object
// that stands for *the same* opaque origin (`Origin.from(origin)`, the global's
// own origin read twice) shares its record.
//
// Platform objects whose origin lives outside this file — a cross-frame
// `WindowProxy` facade, a `MessageEvent` delivered by `postMessage` — register
// an extractor through `_lumen_origin_register_source(obj, fn)`: `fn()` returns
// anything `Origin.from()` accepts (an `Origin`, an absolute URL string, this
// realm's global), or `null` for "this object has no origin you may see" (a
// `TypeError` for the caller). A constructed
// `MessageEvent` never gets one, which is exactly why `Origin.from()` throws
// for it: its `.origin` is author-supplied, not a real origin.
(function() {
    var slots = new WeakMap();
    var sources = new WeakMap();
    var nextOpaqueId = 1;
    var selfRecord = null;

    function opaqueRecord() { return { opaque: true, id: nextOpaqueId++ }; }

    // `null` for a string that does not parse as an absolute URL.
    function recordForHref(href) {
        var o = _lumen_url_origin(String(href));
        if (o === null || o === undefined) return null;
        if (o.opaque) return opaqueRecord();
        return { opaque: false, scheme: o.scheme, host: o.host, port: o.port, site: o.site };
    }

    function wrap(record) {
        var o = Object.create(Origin.prototype);
        slots.set(o, record);
        return o;
    }

    function recordOf(value, what) {
        var r = (value !== null && typeof value === 'object') ? slots.get(value) : undefined;
        if (r === undefined) {
            throw new TypeError("Failed to execute '" + what + "' on 'Origin': parameter 1 is not of type 'Origin'.");
        }
        return r;
    }

    // The origin of this realm's own global: its document's URL on a page, its
    // script URL in a worker. Read once and kept, so an opaque global origin
    // (a `data:` document or worker) stays same-origin with itself.
    //
    // BUG-1208: NOT `location.href` — for a non-sandboxed `about:blank`/
    // `about:srcdoc` document `location.href` is still the `about:` address
    // itself (opaque), while the realm's own origin is inherited from the
    // PARENT (HTML LS §7.4.1). `_LUMEN_ORIGIN` (Rust, `v8_runtime.rs::install_dom`'s
    // `realm_origin`) already carries that distinction — a serialized tuple
    // origin (`scheme://host[:port]`) or the literal `"null"` for an opaque
    // one (a sandboxed frame's fresh, per-document identity included, since
    // Rust already minted the sandboxed case's opaque-ness the same way
    // `location`'s own origin would). A serialized tuple origin string is
    // itself a valid absolute URL of that origin, so `recordForHref` maps it
    // straight back to the same tuple record `URL.prototype.origin` would
    // give for that address — no separate parser needed.
    function globalRecord() {
        if (selfRecord === null) {
            var origin = typeof _LUMEN_ORIGIN !== 'undefined' ? String(_LUMEN_ORIGIN) : null;
            if (origin !== null) {
                selfRecord = (origin && origin !== 'null') ? recordForHref(origin) : null;
            } else {
                // A worker scope: `_LUMEN_ORIGIN` is a page-only global
                // (`install_dom`) — no `about:`-inheritance case exists for a
                // worker, so its own script URL is always the right answer.
                var href = (typeof location !== 'undefined' && location) ? String(location.href) : '';
                selfRecord = recordForHref(href);
            }
            if (selfRecord === null) selfRecord = opaqueRecord();
        }
        return selfRecord;
    }

    // HTML `<a>`/`<area>` (HTMLHyperlinkElementUtils), SVG `<a>` (`href`, then
    // `xlink:href`) and MathML `<a>`: the element's `href` resolved against the
    // document base URL. No `href` at all, or one that does not parse, is "no
    // origin".
    var XLINK_NS = 'http://www.w3.org/1999/xlink';
    function hyperlinkHref(value) {
        var isHtml = (typeof HTMLAnchorElement === 'function' && value instanceof HTMLAnchorElement)
            || (typeof HTMLAreaElement === 'function' && value instanceof HTMLAreaElement);
        var isSvg = typeof SVGAElement === 'function' && value instanceof SVGAElement;
        var isMathml = typeof MathMLElement === 'function' && value instanceof MathMLElement
            && value.localName === 'a';
        if (!isHtml && !isSvg && !isMathml) return undefined;
        var raw = value.getAttribute('href');
        if (raw === null && isSvg) raw = value.getAttributeNS(XLINK_NS, 'href');
        if (raw === null) return null;
        var base = (typeof document !== 'undefined' && document) ? document.baseURI : undefined;
        var parsed = _lumen_url_parse(String(raw), base ? String(base) : undefined);
        return parsed ? parsed.href : null;
    }

    function recordFrom(value) {
        if (typeof value === 'string') return recordForHref(value);
        if (value === null || (typeof value !== 'object' && typeof value !== 'function')) return null;
        var own = slots.get(value);
        if (own !== undefined) return own;
        if (value === globalThis) return globalRecord();
        var fn = sources.get(value);
        if (fn !== undefined) {
            var got = fn();
            return (got === null || got === undefined || got === value) ? null : recordFrom(got);
        }
        if (typeof URL === 'function' && value instanceof URL) return recordForHref(value.href);
        var href = hyperlinkHref(value);
        if (typeof href === 'string') return recordForHref(href);
        return null;
    }

    class Origin {
        constructor() { slots.set(this, opaqueRecord()); }

        static from(value) {
            var r = recordFrom(value);
            if (r === null) {
                throw new TypeError("Failed to execute 'from' on 'Origin': the value has no origin.");
            }
            return wrap(r);
        }

        get opaque() { return recordOf(this, 'opaque').opaque; }

        isSameOrigin(other) {
            var a = recordOf(this, 'isSameOrigin');
            var b = recordOf(other, 'isSameOrigin');
            if (a.opaque || b.opaque) return a.opaque && b.opaque && a.id === b.id;
            return a.scheme === b.scheme && a.host === b.host && a.port === b.port;
        }

        // HTML LS §7.1.1 «same site»: opaque origins only with themselves,
        // tuple origins when scheme and registrable domain (host for an IP
        // address) agree — schemeful, port-insensitive.
        isSameSite(other) {
            var a = recordOf(this, 'isSameSite');
            var b = recordOf(other, 'isSameSite');
            if (a.opaque || b.opaque) return a.opaque && b.opaque && a.id === b.id;
            return a.scheme === b.scheme && a.site === b.site;
        }
    }
    Object.defineProperty(Origin.prototype, Symbol.toStringTag, { value: 'Origin', configurable: true });
    Object.defineProperty(globalThis, 'Origin', {
        value: Origin, writable: true, enumerable: false, configurable: true,
    });
    globalThis._lumen_origin_register_source = function(obj, fn) { sources.set(obj, fn); };
    // BUG-1198: an opaque origin minted outside this realm (a sandboxed
    // frame's document, `frame_bridge.rs::next_opaque_origin_id`), keyed by
    // that identity — the same key yields the same record, so two messages of
    // one sandboxed document are same-origin and a reloaded one is not.
    var foreignOpaque = new Map();
    globalThis._lumen_origin_opaque = function(key) {
        var r = foreignOpaque.get(key);
        if (r === undefined) {
            r = opaqueRecord();
            foreignOpaque.set(key, r);
        }
        return wrap(r);
    };
})();
