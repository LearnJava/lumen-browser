
// Expose new globals on window object (defined after window literal because
// `var performance` is not hoisted with its value, only its name).
window.URL                   = URL;
window.URLSearchParams       = URLSearchParams;
window.performance           = performance;
window.queueMicrotask        = queueMicrotask;
window.Event                 = Event;
window.CustomEvent           = CustomEvent;
window.UIEvent               = UIEvent;
window.MouseEvent            = MouseEvent;
window.KeyboardEvent         = KeyboardEvent;
window.InputEvent            = InputEvent;
window.FocusEvent            = FocusEvent;
window.WheelEvent            = WheelEvent;
window.PointerEvent          = PointerEvent;
window.AnimationEvent        = AnimationEvent;
window.TransitionEvent       = TransitionEvent;
window.Animation             = Animation;
window.KeyframeEffect        = KeyframeEffect;
window.DocumentTimeline      = DocumentTimeline;
window.AnimationPlaybackEvent = AnimationPlaybackEvent;
window.StorageEvent          = StorageEvent;
window.PopStateEvent         = PopStateEvent;
window.HashChangeEvent       = HashChangeEvent;
window.ErrorEvent            = ErrorEvent;
window.PromiseRejectionEvent = PromiseRejectionEvent;
window.SubmitEvent           = SubmitEvent;
window.PageTransitionEvent   = PageTransitionEvent;
window.BeforeUnloadEvent     = BeforeUnloadEvent;
window.DataTransfer          = DataTransfer;
window.DataTransferItem      = DataTransferItem;
window.DataTransferItemList  = DataTransferItemList;
window.DragEvent             = DragEvent;
window.ClipboardEvent        = ClipboardEvent;
window.CompositionEvent      = CompositionEvent;
window.scheduler                = scheduler;
window.requestIdleCallback      = requestIdleCallback;
window.cancelIdleCallback       = cancelIdleCallback;
window.ValidityState            = ValidityState;
window.setTimeout            = setTimeout;
window.clearTimeout          = clearTimeout;
window.setInterval           = setInterval;
window.clearInterval         = clearInterval;
window.MutationObserver      = MutationObserver;
window.MutationRecord        = MutationRecord;
window.ResizeObserver        = ResizeObserver;
window.IntersectionObserver  = IntersectionObserver;
window.HTMLCollection        = HTMLCollection;
window.NodeList              = NodeList;
window.NodeFilter            = NodeFilter;
window.TreeWalker            = _TreeWalker;
window.NodeIterator          = _NodeIterator;
window.Performance           = Performance;
window.PerformanceObserver   = PerformanceObserver;
window.MediaQueryList        = MediaQueryList;
window.MediaQueryListEvent   = MediaQueryListEvent;
// CSS Media Queries L4 §4.2 — Window.matchMedia returns a live MediaQueryList.
// Bare `matchMedia(...)` (without window prefix) also works because the var
// declaration below promotes it to a global.
var matchMedia = function(media) {
    var mql = new MediaQueryList(media);
    _mqlRegistry.push(mql);
    return mql;
};
window.matchMedia            = matchMedia;

// ── window scroll API (CSSOM View Module §4) ────────────────────────────────
// window.scrollX / scrollY / pageXOffset / pageYOffset — read current page scroll.
// window.scrollTo / scroll / scrollBy — programmatic page scroll with behavior option.
Object.defineProperties(window, {
    scrollY: { get: function() { return _lumen_get_page_scroll_y(); }, enumerable: true },
    scrollX: { get: function() { return 0; }, enumerable: true },
    pageYOffset: { get: function() { return _lumen_get_page_scroll_y(); }, enumerable: true },
    pageXOffset: { get: function() { return 0; }, enumerable: true }
});
// BUG-479: both now return a Promise (CSSOM View's "Scrolling with a
// promise" revision) settled through `_lumen_scroll_settle_promise`
// (`web_api_shim_head.js`) — resolves once the requested scroll's own
// `scrollend` lands, or immediately if the request never moved the page at
// all (native drops `scroll`/`scrollend` entirely for a no-op page scroll,
// unlike the element/container path, so waiting on the event alone would
// hang forever on e.g. `scrollTo(scrollX, scrollY)`).
window.scrollTo = function(x, y) {
    var top, smooth;
    if (typeof x === 'object' && x !== null) { top = +(x.top || 0); smooth = x.behavior === 'smooth' ? 1 : 0; }
    else { top = +(y || 0); smooth = 0; }
    _lumen_request_page_scroll(top, smooth);
    return _lumen_scroll_settle_promise(window, function() { return [0, _lumen_get_page_scroll_y()]; });
};
window.scroll = window.scrollTo;
window.scrollBy = function(x, y) {
    var dy, smooth;
    if (typeof x === 'object' && x !== null) { dy = +(x.top || 0); smooth = x.behavior === 'smooth' ? 1 : 0; }
    else { dy = +(y || 0); smooth = 0; }
    _lumen_request_page_scroll(_lumen_get_page_scroll_y() + dy, smooth);
    return _lumen_scroll_settle_promise(window, function() { return [0, _lumen_get_page_scroll_y()]; });
};

// ── window.CSS (CSS Object Model L1 §5 + CSS Conditional Rules L3 §6) ────────
// CSS.supports(property, value) — two-argument form.
// CSS.supports(conditionText) — one-argument form.
// CSS.escape(ident) — CSS.escape() L1 §4.2 (WhatWG CSS OM).
var CSS = {
    supports: function(prop, value) {
        if (arguments.length < 2) {
            // One-argument form: CSS.supports(conditionText)
            // Strip outermost parens if present (common usage pattern).
            var cond = String(prop);
            return !!_lumen_css_supports_cond(cond);
        }
        // Two-argument form: CSS.supports(property, value)
        return !!_lumen_css_supports_prop(String(prop), String(value));
    },
    escape: function(ident) {
        // CSS.escape() — WhatWG CSS OM §4.2.
        // Escapes all chars that are not safe in CSS identifiers.
        ident = String(ident);
        var result = '';
        for (var i = 0; i < ident.length; i++) {
            var code = ident.charCodeAt(i);
            var ch = ident[i];
            if (i === 0 && code >= 0x30 && code <= 0x39) {
                // Leading digit (escape as hex) — escape as hex.
                result += '\\' + code.toString(16) + ' ';
                continue;
            }
            // Safe: [a-zA-Z0-9_-] and non-ASCII.
            if ((code >= 0x61 && code <= 0x7a) ||
                (code >= 0x41 && code <= 0x5a) ||
                (code >= 0x30 && code <= 0x39) ||
                code === 0x5f || code === 0x2d || code >= 0x80) {
                result += ch;
            } else if (code === 0x00) {
                result += '�';
            } else if (code <= 0x1f || code === 0x7f) {
                result += '\\' + code.toString(16) + ' ';
            } else {
                result += '\\' + ch;
            }
        }
        return result;
    },
};
window.CSS = CSS;

window.Blob                  = Blob;
window.File                  = File;
window.FileReader            = FileReader;
window.btoa                  = btoa;
window.atob                  = atob;
window.MessageChannel        = MessageChannel;
window.MessagePort           = MessagePort;
// W3C Secure Contexts: computed from the document URL by
// `_lumen_url_is_potentially_trustworthy` (BUG-399) — see there for the rules.
// Snapshotted here, not re-read from `_lumen_loc_parts` on every access: the
// flag belongs to the environment settings object and is fixed when the
// document is created (HTML LS §8.1.5.1), and a document is exactly what a
// fresh runtime is installed for (`install_dom` runs per navigation). Until
// BUG-829 a live read would also have been actively wrong — `pushState` stored
// its raw relative argument in `_lumen_loc_parts`, which flipped the flag to
// false on an https page; that string is an absolute URL of the same origin
// now, so the snapshot rests on the spec rule alone rather than on working
// around a defect. The value is held in a closure, not in a
// `_lumen_…` global, so it survives `seal_internal_globals_v8` leaving engine
// *state* writable; the property itself is the readonly accessor WebIDL
// declares, so page script cannot answer for the engine by plain assignment.
(function() {
    var secure = _lumen_url_is_potentially_trustworthy(_lumen_loc_parts);
    Object.defineProperty(window, 'isSecureContext', {
        get: function() { return secure; },
        enumerable: true, configurable: true,
    });
})();
// Set by Rust via _LUMEN_CROSS_ORIGIN_ISOLATED global (COOP=same-origin + COEP=require-corp).
window.crossOriginIsolated   = !!_LUMEN_CROSS_ORIGIN_ISOLATED;

// ── window.open() (HTML LS §8.7.1) ─────────────────────────────────────────
// Opens a new browsing context (implemented as a new tab in Lumen).
// Returns a stub WindowProxy with location/close — actual cross-window state
// sharing is not implemented (window.opener is always null).
window.opener = null;
window.open = function(url, target, features) {
  url     = (url     == null) ? '' : String(url);
  target  = (target  == null) ? '_blank' : String(target);
  features = (features == null) ? '' : String(features);
  if (url !== '') {
    try { url = new URL(url, _lumen_loc_href).href; } catch (e) {}
  }
  _lumen_window_open(url, target, features);
  // Return a minimal stub so callers can call .close() / read .location.href
  // without throwing. Real cross-window messaging is not yet supported.
  var href = url || 'about:blank';
  return {
    closed: false,
    opener: null,
    name: target,
    location: {
      href: href,
      toString: function() { return href; }
    },
    close: function() { this.closed = true; },
    focus: function() {},
    blur: function() {},
    postMessage: function() {}
  };
};
window.close = function() {};

// ── Lazy image loading (HTML LS §2.6.6.9) ──────────────────────────────────
// Maps nid (u32 as string key) → url for images deferred by loading="lazy".
// Internal IntersectionObserver for lazy images (HTML LS loading=lazy, §lazy-loading).
// Created on first _lumen_init_lazy_images call; uses rootMargin to load images
// one viewport-height ahead of the visible area.
var _lazy_io = null;
// nid → url for images not yet loaded; populated by _lumen_init_lazy_images.
var _lazy_io_urls = {};

// Called by shell after initial layout with [[nid, url], ...] for lazy images.
// Creates an internal IntersectionObserver that fires _lumen_request_lazy_image_load
// when each image enters the lazy-load margin. Idempotent: re-registration skipped.
function _lumen_init_lazy_images(pairs) {
    if (pairs.length === 0) return;
    if (!_lazy_io) {
        var vp = _lumen_get_viewport_size();
        // HTML LS §lazy-loading distance threshold: load 1 viewport-height ahead.
        var margin = Math.round(vp[1]);
        _lazy_io = new IntersectionObserver(function(entries) {
            for (var i = 0; i < entries.length; i++) {
                var entry = entries[i];
                if (!entry.isIntersecting) continue;
                var nid = entry.target.__nid__;
                if (_lazy_io_urls[nid] !== undefined) {
                    _lumen_request_lazy_image_load(nid, _lazy_io_urls[nid]);
                    delete _lazy_io_urls[nid];
                    _lazy_io.unobserve(entry.target);
                }
            }
        }, { rootMargin: '0px 0px ' + margin + 'px 0px' });
    }
    for (var i = 0; i < pairs.length; i++) {
        var nid = pairs[i][0];
        if (_lazy_io_urls[nid] === undefined) {
            _lazy_io_urls[nid] = pairs[i][1];
            // Proxy object: IntersectionObserver only needs __nid__ to look up the rect.
            _lazy_io.observe({ __nid__: nid });
        }
    }
}

// Called by shell after each relayout.  Lazy images are now delivered via
// _lazy_io (an IntersectionObserver), which fires inside
// _lumen_deliver_intersection_observers() called earlier by deliver_layout_observers().
// This function is kept for shell API compatibility.
function _lumen_deliver_lazy_images() {}
