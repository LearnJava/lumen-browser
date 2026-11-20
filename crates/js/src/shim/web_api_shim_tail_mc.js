
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
window.IntersectionObserverEntry = IntersectionObserverEntry;
window.HTMLCollection        = HTMLCollection;
window.NodeList              = NodeList;
window.NodeFilter            = NodeFilter;
window.TreeWalker            = _TreeWalker;
window.NodeIterator          = _NodeIterator;
window.Performance           = Performance;
window.PerformanceObserver   = PerformanceObserver;
window.PerformanceObserverEntryList = PerformanceObserverEntryList;
window.PerformancePaintTiming = PerformancePaintTiming;
window.LargestContentfulPaint = LargestContentfulPaint;
window.PerformanceResourceTiming = PerformanceResourceTiming;
window.PerformanceNavigationTiming = PerformanceNavigationTiming;
window.LayoutShift           = LayoutShift;
window.LayoutShiftAttribution = LayoutShiftAttribution;
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
    scrollX: { get: function() { return _lumen_get_page_scroll_x(); }, enumerable: true },
    pageYOffset: { get: function() { return _lumen_get_page_scroll_y(); }, enumerable: true },
    pageXOffset: { get: function() { return _lumen_get_page_scroll_x(); }, enumerable: true }
});

// ── window.innerWidth/innerHeight/outerWidth/outerHeight (CSSOM View §5.3) ──
// inner* is the CSS-pixel viewport size, already computed for scroll/layout
// via _lumen_get_viewport_size(). BUG-529: these were absent from the shim
// entirely. There is no window chrome model in this single-window shell, so
// outer* aliases the same size (the same simplification already used for
// other window-chrome-less properties, e.g. visualViewport above).
Object.defineProperties(window, {
    innerWidth:  { get: function() { return _lumen_get_viewport_size()[0]; }, enumerable: true },
    innerHeight: { get: function() { return _lumen_get_viewport_size()[1]; }, enumerable: true },
    outerWidth:  { get: function() { return _lumen_get_viewport_size()[0]; }, enumerable: true },
    outerHeight: { get: function() { return _lumen_get_viewport_size()[1]; }, enumerable: true }
});

// ── window.devicePixelRatio (CSSOM View §4) ─────────────────────────────────
// BUG-515: was undefined on an ordinary page — the only assignment lived in
// the opt-in getScreenDetails() path (window_management.rs), which never
// runs unless a page calls it. Fixed 1 is spec-conformant here: this engine's
// rendering pipeline is not HiDPI-aware, so there is no real per-monitor
// ratio to report yet.
// UX-ZOOM: browser page zoom scales the ratio (CSSOM View §4: "the number of
// device pixels per CSS pixel", which a page zoom changes), so it follows
// `_lumen_get_zoom_factor` — 1 at 100%, 1.5 at 150%.
if (typeof globalThis.devicePixelRatio === 'undefined') {
    Object.defineProperty(globalThis, 'devicePixelRatio', {
        get: function() { return _lumen_get_zoom_factor(); },
        set: function(v) {
            Object.defineProperty(globalThis, 'devicePixelRatio',
                { value: v, writable: true, configurable: true, enumerable: true });
        },
        configurable: true, enumerable: true
    });
}
// BUG-479: both now return a Promise (CSSOM View's "Scrolling with a
// promise" revision) settled through `_lumen_scroll_settle_promise`
// (`web_api_shim_head.js`) — resolves once the requested scroll's own
// `scrollend` lands, or immediately if the request never moved the page at
// all (native drops `scroll`/`scrollend` entirely for a no-op page scroll,
// unlike the element/container path, so waiting on the event alone would
// hang forever on e.g. `scrollTo(scrollX, scrollY)`).
// SMOOTH-SCROLL: both axes, `{left, top, behavior}`, and `scroll-behavior` of
// the root element for `behavior: "auto"` (`_lumen_scroll_to_pos`).
function _lumen_window_scroll(args, relative, fn) {
    var a = _lumen_parse_scroll_args(args, [_lumen_get_page_scroll_x(), _lumen_get_page_scroll_y()], relative, fn);
    _lumen_scroll_to_pos('p', a.x, a.y, a.behavior);
    return _lumen_scroll_settle_promise(window, function() {
        return [_lumen_get_committed_page_scroll_x(), _lumen_get_committed_page_scroll_y()];
    }, 'p');
}
window.scrollTo = function() { return _lumen_window_scroll(arguments, false, 'scrollTo'); };
window.scroll = window.scrollTo;
window.scrollBy = function() { return _lumen_window_scroll(arguments, true, 'scrollBy'); };

// ── window.visualViewport (Visual Viewport API) ─────────────────────────────
// `scale` is `<meta name=viewport initial-scale>` (`_lumen_get_meta_viewport_scale`,
// GAP-VVPORT срез 3) — real page zoom (Ctrl+=/Ctrl+-/Ctrl+0,
// `_lumen_get_zoom_factor`) reflows the *layout* viewport itself
// (`Lumen::relayout_viewport`), so layout and visual viewport stay equal
// under it and it does not feed `scale` here, matching how desktop browser
// zoom leaves `visualViewport.scale` at 1 and only pinch-zoom moves it.
// width/height shrink by that same `scale`, since the visual viewport is the
// portion of the layout viewport actually visible. Pinch-zoom panning is not
// modeled (no touch input), so offsetLeft/offsetTop/pageLeft stay 0 — the
// visual viewport's top-left always coincides with the layout viewport's.
// GAP-VVPORT: `resize`/`scroll` dispatch for real —
// `_lumen_fire_window_resize_event`/`_lumen_fire_window_scroll_event`
// (`web_api_shim_tail_b.js`) also fire on this object. `onresize`/`onscroll`
// are plain EventTarget listeners, invoked through the generic
// `this['on'+type]` branch in `EventTarget.prototype.dispatchEvent`
// (`event_target_shim.js`) — no separate wiring needed. `onscrollend` stays
// undelivered: the Visual Viewport API declares no `scrollend`.
function VisualViewport() {
    EventTarget.call(this);
    this.onresize = null;
    this.onscroll = null;
    this.onscrollend = null;
}
VisualViewport.prototype = Object.create(EventTarget.prototype);
VisualViewport.prototype.constructor = VisualViewport;
Object.defineProperties(VisualViewport.prototype, {
    width:      { get: function() { return _lumen_get_viewport_size()[0] / _lumen_get_meta_viewport_scale(); } },
    height:     { get: function() { return _lumen_get_viewport_size()[1] / _lumen_get_meta_viewport_scale(); } },
    offsetLeft: { get: function() { return 0; } },
    offsetTop:  { get: function() { return 0; } },
    pageLeft:   { get: function() { return _lumen_get_page_scroll_x(); } },
    pageTop:    { get: function() { return _lumen_get_page_scroll_y(); } },
    scale:      { get: function() { return _lumen_get_meta_viewport_scale(); } }
});
window.visualViewport = new VisualViewport();

// ── BarProp: window.locationbar/menubar/personalbar/scrollbars/statusbar/toolbar
// HTML LS §7.2.4 (BUG-1143). Six distinct objects, one per attribute, each
// with a `visible` getter that is the negation of the top-level traversable's
// "is popup". Lumen opens every `window.open()` target as a full tab with the
// browser UI, so no browsing context here is ever a popup and `visible` is
// always `true`. Each attribute is `[Replaceable]`: an own accessor of
// `window` whose setter shadows it with a plain data property (WebIDL
// §3.7.6), the shape `window-properties.https.html` checks. The accessors are
// carried onto `globalThis` by the descriptor-preserving copy in
// `web_api_shim_tail_b.js`, so the bare identifiers (`toolbar`) resolve too.
function BarProp() { throw new TypeError('Illegal constructor'); }
Object.defineProperty(BarProp.prototype, 'visible', {
    get: function() { return true; },
    enumerable: true, configurable: true,
});
Object.defineProperty(BarProp.prototype, Symbol.toStringTag, { value: 'BarProp', configurable: true });
['locationbar', 'menubar', 'personalbar', 'scrollbars', 'statusbar', 'toolbar'].forEach(function(name) {
    var bar = Object.create(BarProp.prototype);
    Object.defineProperty(window, name, {
        get: function() { return bar; },
        set: function(v) {
            Object.defineProperty(globalThis, name, { value: v, writable: true, enumerable: true, configurable: true });
        },
        enumerable: true, configurable: true,
    });
});

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
// BUG-765: reuses `_lumen_secure_context` (computed once, `web_api_shim_mid_b.js`)
// instead of recomputing it — that same variable is what gates every
// `[SecureContext]` surface installed after this file.
(function() {
    var secure = _lumen_secure_context;
    Object.defineProperty(window, 'isSecureContext', {
        get: function() { return secure; },
        enumerable: true, configurable: true,
    });
})();
// Set by Rust via _LUMEN_CROSS_ORIGIN_ISOLATED global (COOP=same-origin + COEP=require-corp).
window.crossOriginIsolated   = !!_LUMEN_CROSS_ORIGIN_ISOLATED;

// ── window.open() (HTML LS §8.7.1) ─────────────────────────────────────────
// Opens a new browsing context (implemented as a new tab in Lumen).
// Returns a stub WindowProxy with location/close/postMessage.
//
// GAP-NAVCTX срез 4 (BUG-797): `postMessage` used to be a permanent no-op —
// `window.opener` on the popup side was always `null` too — because nothing
// addressed the popup's tab before the shell had even created it. The token
// `_lumen_window_open` now returns fixes that: it is a stable handle the
// shell resolves to the popup's real tab id on its next tick
// (`window_messaging::resolve_token`, Rust), so a `.postMessage()` call made
// before that resolution still queues correctly. `_lumen_win_stubs` keeps
// this object reachable by token so a reply delivered from the popup can set
// `MessageEvent.source` back to this exact object (see
// `_lumen_window_pump_messages` below and `_lumen_install_opener`).
window.opener = null;
var _lumen_win_stubs = {};
// Set by `_lumen_install_opener` below when this tab is a popup; -1 (no
// opener) otherwise. Declared here so a page that queries them before ever
// being installed as a popup sees a defined default, not a ReferenceError.
var _lumen_own_tab_id = -1;
var _lumen_opener_tab_id = -1;
window.open = function(url, target, features) {
  url     = (url     == null) ? '' : String(url);
  target  = (target  == null) ? '_blank' : String(target);
  features = (features == null) ? '' : String(features);
  if (url !== '') {
    try { url = new URL(url, _lumen_loc_href).href; } catch (e) {}
  }
  // UX-USER-ACTIVATION: a new browsing context needs a live user activation
  // (HTML LS 6.4.3, popup blocking). The activation is consumed, so a second
  // open() from the same gesture is blocked; a blocked call returns null (the
  // shell lights the blocked-popup indicator). `_self` navigates the caller
  // and is not a popup. Inert unless the shell turned the gate on and the
  // site is not allowed to open popups.
  if (target.toLowerCase() !== '_self' && typeof _lumen_gesture_required === 'function'
      && _lumen_gesture_required(String(location.origin), 'popups')) {
    if (typeof _lumen_user_activation_is_active === 'function' && _lumen_user_activation_is_active()) {
      _lumen_consume_user_activation();
    } else {
      _lumen_popup_blocked(url, target);
      return null;
    }
  }
  var token = _lumen_window_open(url, target, features);
  var href = url || 'about:blank';
  var stub = {
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
    postMessage: function(message) {
      var json;
      try {
        json = JSON.stringify(message === undefined ? null : message);
        if (json === undefined) json = 'null';
      } catch (e) {
        throw new DOMException("Failed to execute 'postMessage' on 'Window': value could not be cloned.", "DataCloneError");
      }
      _lumen_window_postmessage_to_token(token, json, location.origin);
    }
  };
  _lumen_win_stubs[token] = stub;
  return stub;
};

// Installs the real `window.opener` handle on a popup tab. `run_scripts_with_dom`
// (Rust, `scripts.rs`) now calls this itself, before the popup's first
// top-level script line, whenever the shell armed a pending opener pair for
// this load (GAP-NAVCTX срез 5, BUG-797) — so a synchronous top-of-page
// `opener.postMessage()` sees the real handle. The shell (`about_to_wait`)
// also still calls this once more after `Lumen::navigate_to` returns, with
// the same two ids — a no-op re-assignment in the common case, and the only
// path that runs at all for a popup document with no scripts (which never
// reaches `run_scripts_with_dom`'s runtime creation). See
// `crate::window_messaging` (Rust) for the tab-id addressing scheme this feeds.
__lumen_C._lumen_install_opener = function(ownTabId, openerTabId) {
  _lumen_own_tab_id = ownTabId;
  _lumen_opener_tab_id = openerTabId;
  window.opener = {
    closed: false,
    postMessage: function(message) {
      var json;
      try {
        json = JSON.stringify(message === undefined ? null : message);
        if (json === undefined) json = 'null';
      } catch (e) {
        throw new DOMException("Failed to execute 'postMessage' on 'Window': value could not be cloned.", "DataCloneError");
      }
      _lumen_window_postmessage_to_opener(openerTabId, ownTabId, json, location.origin);
    }
  };
};

// Delivers messages the shell drained from `window_messaging`'s hub for this
// tab (`tabId`, a plain number the shell already knows — see the module docs
// for why this is a shell-driven pull rather than a self-pumping tick the
// way `_lumen_frame_pump_messages` is). Reuses `_lumen_deliver_frame_message`
// for the actual `MessageEvent` construction/dispatch (onmessage, then
// `addEventListener('message', …)`), same as the cross-frame bridge.
__lumen_C._lumen_window_pump_messages = function(tabId) {
  if (typeof _lumen_window_take_messages !== 'function') return;
  var raw = _lumen_window_take_messages(tabId);
  if (!raw) return;
  var msgs;
  try { msgs = JSON.parse(raw); } catch (e) { return; }
  if (typeof _lumen_deliver_frame_message !== 'function') return;
  for (var i = 0; i < msgs.length; i++) {
    var m = msgs[i];
    var source = m.fromOpener ? (window.opener || null) : (_lumen_win_stubs[m.token] || null);
    _lumen_deliver_frame_message(m.data, m.origin, source);
  }
};
// ── window.close()/closed/name (HTML LS §7.2.2, §7.4.5-§7.4.6) ─────────────
// `name` is a plain Window attribute defaulting to the empty string (BUG-887);
// assignment already worked before this fix (it read back whatever was last
// set), the defect was purely the missing default.
window.name = '';
// `closed` is spec-readonly, but a getter-only accessor on the global object
// (`window` becomes `globalThis`, see comment above `isSecureContext`) breaks
// any top-level `var closed = ...;` in later script: PutValue on an accessor
// with no setter is a silent no-op, so the variable can never change again.
// Real WPT pages hit that as a spec-correct footgun; our own test fixtures
// use `closed` as an ordinary local-ish name and are not testing
// unforgeability, so — like `opener`/`name` above — this stays a plain
// assignable property rather than a strict accessor.
window.closed = false;
// close() runs the same prompt-to-unload + unload steps a real navigation
// triggers from the shell (`_lumen_fire_beforeunload`/`_lumen_unload_document`,
// see `persistent_js.rs`), then flips `closed`. It does not ask the shell to
// tear down the tab — this engine has one JS context per page, and script
// keeps running after `close()` returns (BUG-887's minimum fix; actually
// discarding the browsing context is out of scope here).
window.close = function() {
    if (window.closed) return;
    _lumen_fire_beforeunload();
    _lumen_unload_document(false);
    window.closed = true;
};

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

// ── Lazy loading for <audio>/<video> (BUG-925, HTML LS §4.8.11) ────────────
// Unlike images, a media element's resource selection algorithm is driven
// entirely from JS (`audio_element.rs`/`video_element.js`), so deferring it
// needs no shell-side candidate list — a real IntersectionObserver on the
// element itself is enough. `_lumen_media_is_rendered` implements the
// "being rendered" half of the gate: a media element that is disconnected,
// `hidden`, `display:none`, or (audio only) has no `controls` attribute
// must never start loading, not even once it becomes visible.
function _lumen_media_is_rendered(el, requiresControls) {
    if (!el.isConnected) return false;
    if (el.hasAttribute && el.hasAttribute('hidden')) return false;
    if (requiresControls && !(el.hasAttribute && el.hasAttribute('controls'))) return false;
    try {
        var cs = (typeof getComputedStyle === 'function') ? getComputedStyle(el) : null;
        if (cs && cs.display === 'none') return false;
    } catch (e) {}
    return true;
}

// One IntersectionObserver per pending element, keyed by nid. A second call
// before the first delivers just swaps the callback (the caller passed a
// newer URL) instead of stacking a second observer on the same target.
var _lazy_media_io = {};
function _lumen_defer_lazy_media_load(el, onVisible) {
    var nid = el.__nid__;
    if (nid === undefined) return;
    var io = _lazy_media_io[nid];
    if (io) { io._onVisible = onVisible; return; }
    io = new IntersectionObserver(function(entries) {
        for (var i = 0; i < entries.length; i++) {
            if (entries[i].isIntersecting) io._onVisible();
        }
    });
    io._onVisible = onVisible;
    _lazy_media_io[nid] = io;
    io.observe(el);
}
function _lumen_cancel_lazy_media_load(el) {
    var nid = el.__nid__;
    var io = _lazy_media_io[nid];
    if (io) { io.disconnect(); delete _lazy_media_io[nid]; }
}

// ── RTCPeerConnection (WebRTC) — BUG-1829 / BUG-1835 ────────────────────────
// Lumen has no ICE/DTLS/SCTP stack. For anti-bot parity (Ozon abt collects
// RTCPeerConnection shape + createOffer/createDataChannel behaviour) the
// interface now answers like Chrome's surface: full prototype method/attribute
// inventory, SDP answers that parse, data channels that open. The transport
// itself is inert — `send()` drops payloads and no network traffic exists;
// that limitation stays honest in that negotiation never produces connectivity.
// [SecureContext] + [Exposed=Window], like Chrome.
(function() {
    if (typeof _lumen_secure_context === 'undefined' || _lumen_secure_context === false) return;
    if (typeof EventTarget !== 'function') return;

    var _rtcStates = new WeakMap();

    function _rtcRandHex(n) {
        var s = '';
        for (var i = 0; i < n; i++) s += '0123456789abcdef'[Math.floor(Math.random() * 16)];
        return s;
    }
    function _rtcFingerprint() {
        var parts = [];
        for (var i = 0; i < 32; i++) parts.push(_rtcRandHex(2).toUpperCase());
        return parts.join(':');
    }
    function _rtcSdp(kind) {
        var ufrag = _rtcRandHex(4), pwd = _rtcRandHex(22);
        var fp = _rtcFingerprint();
        var s = 'v=0\r\no=- ' + Math.floor(Math.random() * 1e10) + ' 2 IN IP4 127.0.0.1\r\n'
            + 's=-\r\nt=0 0\r\n'
            + 'a=group:BUNDLE 0 1\r\n'
            + 'a=extmap-allow-mixed\r\n'
            + 'a=msid-semantic: WMS\r\n'
            + 'm=audio 9 UDP/TLS/RTP/SAVPF 111 63 9 0 8 13 110 126\r\n'
            + 'c=IN IP4 0.0.0.0\r\n'
            + 'a=rtcp:9 IN IP4 0.0.0.0\r\n'
            + 'a=ice-ufrag:' + ufrag + '\r\n'
            + 'a=ice-pwd:' + pwd + '\r\n'
            + 'a=fingerprint:sha-256 ' + fp + '\r\n'
            + 'a=setup:actpass\r\n'
            + 'a=mid:0\r\n'
            + 'a=sendrecv\r\n'
            + 'a=msid:- ' + _rtcRandHex(8) + '\r\n'
            + 'a=rtcp-mux\r\n'
            + 'a=rtcp-rsize\r\n'
            + 'a=rtpmap:111 opus/48000/2\r\n'
            + 'a=rtcp-fb:111 transport-cc\r\n'
            + 'a=fmtp:111 minptime=10;useinbandfec=1\r\n'
            + 'm=video 9 UDP/TLS/RTP/SAVPF 96 97 98 99 100 101 35 36 37 38\r\n'
            + 'c=IN IP4 0.0.0.0\r\n'
            + 'a=rtcp:9 IN IP4 0.0.0.0\r\n'
            + 'a=ice-ufrag:' + ufrag + '\r\n'
            + 'a=ice-pwd:' + pwd + '\r\n'
            + 'a=fingerprint:sha-256 ' + fp + '\r\n'
            + 'a=setup:actpass\r\n'
            + 'a=mid:1\r\n'
            + 'a=sendrecv\r\n'
            + 'a=msid:- ' + _rtcRandHex(8) + '\r\n'
            + 'a=rtcp-mux\r\n'
            + 'a=rtcp-rsize\r\n'
            + 'a=rtpmap:96 VP8/90000\r\n'
            + 'a=rtcp-fb:96 goog-remb\r\n'
            + 'a=rtcp-fb:96 transport-cc\r\n'
            + 'a=rtcp-fb:96 ccm fir\r\n'
            + 'a=rtcp-fb:96 nack\r\n'
            + 'a=rtpmap:97 rtx/90000\r\n'
            + 'a=fmtp:97 apt=96\r\n'
            + 'a=rtpmap:98 VP9/90000\r\n'
            + 'a=rtcp-fb:98 transport-cc\r\n'
            + 'a=rtcp-fb:98 ccm fir\r\n'
            + 'a=rtcp-fb:98 nack\r\n'
            + 'a=fmtp:98 profile-id=0\r\n'
            + 'a=rtpmap:99 rtx/90000\r\n'
            + 'a=fmtp:99 apt=98\r\n'
            + 'a=rtpmap:100 H264/90000\r\n'
            + 'a=rtcp-fb:100 goog-remb\r\n'
            + 'a=rtcp-fb:100 transport-cc\r\n'
            + 'a=rtcp-fb:100 ccm fir\r\n'
            + 'a=rtcp-fb:100 nack\r\n'
            + 'a=fmtp:100 level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f\r\n'
            + 'a=rtpmap:101 rtx/90000\r\n'
            + 'a=fmtp:101 apt=100\r\n';
        return { type: kind, sdp: s };
    }

    var _rtc_pc = function RTCPeerConnection(config) {
        if (!(this instanceof _rtc_pc)) {
            throw new TypeError("Failed to construct 'RTCPeerConnection': "
                + "Please use the 'new' operator, this object constructor cannot be called as a function.");
        }
        EventTarget.call(this);
        _rtcStates.set(this, {
            signalingState: 'stable',
            iceConnectionState: 'new',
            iceGatheringState: 'new',
            connectionState: 'new',
            localDescription: null,
            remoteDescription: null,
            pendingLocalDescription: null,
            pendingRemoteDescription: null,
            currentLocalDescription: null,
            currentRemoteDescription: null,
            sctp: null,
            closed: false,
            handlers: Object.create(null),
            senders: [],
            receivers: [],
            transceivers: [],
            localStreams: [],
            remoteStreams: [],
            dataChannels: [],
            config: config || {}
        });
    };
    Object.setPrototypeOf(_rtc_pc.prototype, EventTarget.prototype);
    var _rtcProto = _rtc_pc.prototype;
    Object.defineProperty(_rtcProto, 'constructor', { value: _rtc_pc, writable: true, configurable: true });
    Object.defineProperty(_rtcProto, Symbol.toStringTag, { value: 'RTCPeerConnection', configurable: true });

    function _rtcCheck(self, name) {
        if (_rtcStates.get(self) === undefined) {
            throw new TypeError("Failed to execute '" + name + "' on 'RTCPeerConnection': Illegal invocation");
        }
    }

    // Readonly attributes — prototype getters over the per-instance state.
    var _rtcAttrs = ['signalingState', 'iceConnectionState', 'iceGatheringState',
        'connectionState', 'localDescription', 'remoteDescription',
        'pendingLocalDescription', 'pendingRemoteDescription',
        'currentLocalDescription', 'currentRemoteDescription', 'sctp'];
    for (var _ri = 0; _ri < _rtcAttrs.length; _ri++) {
        (function(name) {
            Object.defineProperty(_rtcProto, name, {
                get: function() {
                    _rtcCheck(this, name);
                    return _rtcStates.get(this)[name];
                },
                enumerable: true, configurable: true
            });
        })(_rtcAttrs[_ri]);
    }
    Object.defineProperty(_rtcProto, 'canTrickleIceCandidates', {
        get: function() { _rtcCheck(this, 'canTrickleIceCandidates'); return true; },
        enumerable: true, configurable: true
    });

    // `on*` IDL attributes — accessor pair storing the listener.
    var _rtcEvents = ['onicecandidate', 'onicecandidateerror', 'onnegotiationneeded',
        'ontrack', 'onconnectionstatechange', 'oniceconnectionstatechange',
        'onsignalingstatechange', 'onicegatheringstatechange',
        'onaddstream', 'ondatachannel', 'onremovestream'];
    for (var _ei = 0; _ei < _rtcEvents.length; _ei++) {
        (function(name) {
            Object.defineProperty(_rtcProto, name, {
                get: function() {
                    var s = _rtcStates.get(this);
                    return s ? (s.handlers[name] || null) : null;
                },
                set: function(v) {
                    var s = _rtcStates.get(this);
                    if (s === undefined) {
                        throw new TypeError("Failed to set '" + name + "' on 'RTCPeerConnection': Illegal invocation");
                    }
                    if (v !== null && typeof v !== 'function') {
                        throw new TypeError('The provided value is not of type function.');
                    }
                    s.handlers[name] = v;
                },
                enumerable: true, configurable: true
            });
        })(_rtcEvents[_ei]);
    }

    // Negotiation surface — answers like Chrome (SDP parses, promises
    // resolve, signalingState transitions), transport stays inert.
    function _rtcAsyncOk(name, impl) {
        var f = function() {
            var s = _rtcStates.get(this);
            if (s === undefined) {
                throw new TypeError("Failed to execute '" + name + "' on 'RTCPeerConnection': Illegal invocation");
            }
            if (s.closed) {
                return Promise.reject(new DOMException(
                    'RTCPeerConnection.' + name + ': PeerConnection is closed',
                    'InvalidStateError'));
            }
            return impl(s, arguments);
        };
        Object.defineProperty(f, 'name', { value: name });
        Object.defineProperty(f, 'length', { value: 0 });
        return f;
    }
    function _rtcDesc(d) {
        if (d == null) return d;
        if (typeof RTCSessionDescription === 'function') {
            try { return new RTCSessionDescription(d); } catch (_) {}
        }
        return { type: d.type, sdp: d.sdp };
    }
    _rtcProto.createOffer = _rtcAsyncOk('createOffer', function(s) {
        return Promise.resolve(_rtcSdp('offer'));
    });
    _rtcProto.createAnswer = _rtcAsyncOk('createAnswer', function(s) {
        return Promise.resolve(_rtcSdp('answer'));
    });
    _rtcProto.setLocalDescription = _rtcAsyncOk('setLocalDescription', function(s, args) {
        var d = _rtcDesc(args[0]);
        if (d && d.type === 'offer') {
            s.signalingState = 'have-local-offer';
            s.pendingLocalDescription = d;
        } else if (d && d.type === 'answer') {
            s.signalingState = 'stable';
            s.pendingLocalDescription = null;
        } else if (d == null) {
            // Implicit description from a previous createOffer/createAnswer.
            d = _rtcDesc(s.pendingLocalDescription || s.localDescription);
        }
        if (d) {
            s.localDescription = d;
            s.currentLocalDescription = d;
        }
        return Promise.resolve();
    });
    _rtcProto.setRemoteDescription = _rtcAsyncOk('setRemoteDescription', function(s, args) {
        var d = _rtcDesc(args[0]);
        if (d == null) return Promise.resolve();
        if (d.type === 'offer') {
            s.signalingState = 'have-remote-offer';
        } else if (d.type === 'answer') {
            s.signalingState = 'stable';
        }
        s.remoteDescription = d;
        s.currentRemoteDescription = d;
        s.pendingRemoteDescription = null;
        return Promise.resolve();
    });
    _rtcProto.addIceCandidate = _rtcAsyncOk('addIceCandidate', function() {
        return Promise.resolve();
    });
    _rtcProto.getStats = _rtcAsyncOk('getStats', function() {
        return Promise.resolve({});
    });

    // Synchronous queries — backed by the per-instance state.
    _rtcProto.getSenders = function getSenders() { _rtcCheck(this, 'getSenders'); return _rtcStates.get(this).senders.slice(); };
    _rtcProto.getReceivers = function getReceivers() { _rtcCheck(this, 'getReceivers'); return _rtcStates.get(this).receivers.slice(); };
    _rtcProto.getTransceivers = function getTransceivers() { _rtcCheck(this, 'getTransceivers'); return _rtcStates.get(this).transceivers.slice(); };
    _rtcProto.getLocalStreams = function getLocalStreams() { _rtcCheck(this, 'getLocalStreams'); return _rtcStates.get(this).localStreams.slice(); };
    _rtcProto.getRemoteStreams = function getRemoteStreams() { _rtcCheck(this, 'getRemoteStreams'); return _rtcStates.get(this).remoteStreams.slice(); };
    _rtcProto.getConfiguration = function getConfiguration() {
        _rtcCheck(this, 'getConfiguration');
        var c = _rtcStates.get(this).config || {};
        return {
            iceServers: c.iceServers || [],
            iceTransportPolicy: c.iceTransportPolicy || 'all',
            bundlePolicy: c.bundlePolicy || 'max-bundle',
            rtcpMuxPolicy: c.rtcpMuxPolicy || 'require',
            certificates: c.certificates || []
        };
    };
    _rtcProto.setConfiguration = function setConfiguration(config) {
        _rtcCheck(this, 'setConfiguration');
        _rtcStates.get(this).config = config || {};
    };

    // Track/transceiver surface — sender/receiver/transceiver stubs carry
    // the Chrome prototype so `instanceof` checks hold.
    function _rtcSender(track) {
        var s = Object.create(globalThis.RTCRtpSender ? RTCRtpSender.prototype : Object.prototype);
        s.track = track || null;
        s.transport = null;
        return s;
    }
    function _rtcReceiver(track) {
        var s = Object.create(globalThis.RTCRtpReceiver ? RTCRtpReceiver.prototype : Object.prototype);
        s.track = track || null;
        s.transport = null;
        return s;
    }
    function _rtcTransceiver() {
        var s = Object.create(globalThis.RTCRtpTransceiver ? RTCRtpTransceiver.prototype : Object.prototype);
        s.sender = _rtcSender(null);
        s.receiver = _rtcReceiver(null);
        s.direction = 'sendrecv';
        s.currentDirection = null;
        s.mid = null;
        s.stopped = false;
        return s;
    }
    _rtcProto.addTrack = function addTrack(track, streams) {
        _rtcCheck(this, 'addTrack');
        var st = _rtcStates.get(this);
        var sender = _rtcSender(track || null);
        st.senders.push(sender);
        var tr = _rtcTransceiver();
        tr.sender = sender;
        st.transceivers.push(tr);
        if (streams) {
            for (var i = 0; i < streams.length; i++) {
                if (st.localStreams.indexOf(streams[i]) < 0) st.localStreams.push(streams[i]);
            }
        }
        return sender;
    };
    _rtcProto.removeTrack = function removeTrack(sender) {
        _rtcCheck(this, 'removeTrack');
        var st = _rtcStates.get(this);
        var i = st.senders.indexOf(sender);
        if (i >= 0) st.senders.splice(i, 1);
    };
    _rtcProto.addTransceiver = function addTransceiver(trackOrKind, init) {
        _rtcCheck(this, 'addTransceiver');
        var st = _rtcStates.get(this);
        var tr = _rtcTransceiver();
        var kind = typeof trackOrKind === 'string' ? trackOrKind : (trackOrKind && trackOrKind.kind) || 'audio';
        if (typeof trackOrKind === 'object' && trackOrKind !== null) tr.sender.track = trackOrKind;
        tr.sender.track = tr.sender.track || null;
        st.transceivers.push(tr);
        return tr;
    };
    // Legacy stream API — recorded, returns undefined like Chrome.
    _rtcProto.addStream = function addStream(stream) {
        _rtcCheck(this, 'addStream');
        var st = _rtcStates.get(this);
        if (stream && st.localStreams.indexOf(stream) < 0) st.localStreams.push(stream);
    };
    _rtcProto.removeStream = function removeStream(stream) {
        _rtcCheck(this, 'removeStream');
        var st = _rtcStates.get(this);
        var i = st.localStreams.indexOf(stream);
        if (i >= 0) st.localStreams.splice(i, 1);
    };
    _rtcProto.restartIce = function restartIce() {
        _rtcCheck(this, 'restartIce');
        var st = _rtcStates.get(this);
        st.iceConnectionState = 'new';
    };
    _rtcProto.createDTMFSender = function createDTMFSender(track) {
        _rtcCheck(this, 'createDTMFSender');
        return {
            track: track || null,
            duration: 0,
            tones: '',
            ontonechange: null,
            insertDTMF: function() {},
            interrupt: function() {}
        };
    };
    _rtcProto.createDataChannel = function createDataChannel(label, options) {
        var pc = this;
        var st = _rtcStates.get(pc);
        if (st === undefined) {
            throw new TypeError("Failed to execute 'createDataChannel' on 'RTCPeerConnection': Illegal invocation");
        }
        if (st.closed) {
            throw new DOMException('RTCPeerConnection.createDataChannel: PeerConnection is closed', 'InvalidStateError');
        }
        var opts = options || {};
        var ch = {
            label: String(label == null ? '' : label),
            ordered: opts.ordered !== false,
            maxPacketLifeTime: opts.maxPacketLifeTime != null ? opts.maxPacketLifeTime : null,
            maxRetransmits: opts.maxRetransmits != null ? opts.maxRetransmits : null,
            protocol: opts.protocol || '',
            negotiated: !!opts.negotiated,
            id: null,
            readyState: 'connecting',
            bufferedAmount: 0,
            bufferedAmountLowThreshold: 0,
            binaryType: 'blob',
            onopen: null, onclose: null, onmessage: null, onerror: null, onbufferedamountlow: null,
            send: function() {},
            close: function() {
                if (ch.readyState === 'closed') return;
                ch.readyState = 'closed';
                if (typeof ch.onclose === 'function') {
                    try { ch.onclose({ type: 'close', target: ch }); } catch (_) {}
                }
            }
        };
        if (typeof RTCDataChannel === 'function' && RTCDataChannel.prototype) {
            try { Object.setPrototypeOf(ch, RTCDataChannel.prototype); } catch (_) {}
        }
        st.dataChannels.push(ch);
        // Открываем канал микротаской — как Chrome после согласования.
        Promise.resolve().then(function() {
            if (ch.readyState !== 'connecting') return;
            ch.readyState = 'open';
            if (typeof ch.onopen === 'function') {
                try { ch.onopen({ type: 'open', target: ch }); } catch (_) {}
            }
            if (typeof st.handlers.ondatachannel === 'function') {
                try { st.handlers.ondatachannel({ type: 'datachannel', channel: ch, target: pc }); } catch (_) {}
            }
        });
        return ch;
    };

    _rtcProto.close = function close() {
        var s = _rtcStates.get(this);
        if (s === undefined) {
            throw new TypeError("Failed to execute 'close' on 'RTCPeerConnection': Illegal invocation");
        }
        if (s.closed) return;
        s.closed = true;
        s.signalingState = 'closed';
        s.connectionState = 'closed';
        s.iceConnectionState = 'disconnected';
        s.iceGatheringState = 'complete';
    };

    // Static operation — Chrome resolves with a usable certificate.
    _rtc_pc.generateCertificate = function generateCertificate() {
        return Promise.resolve({
            expires: Date.now() + 2592000000,
            getFingerprints: function() { return []; }
        });
    };

    // ── Companion WebRTC interfaces Chrome exposes ──────────────────────
    function _rtcIface(name, protoProps, ctorBody) {
        var f = function() {
            if (new.target === undefined) {
                throw new TypeError("Failed to construct '" + name + "': Please use the 'new' operator.");
            }
            var init = arguments[0] || {};
            var self = ctorBody ? ctorBody.call(this, init) : {};
            return self;
        };
        var p = {};
        (protoProps || []).forEach(function(m) {
            p[m] = function() {
                return Promise.reject(new DOMException(name + '.' + m + ': not supported', 'NotSupportedError'));
            };
        });
        f.prototype = Object.assign(Object.create(Object.prototype), p);
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: name, configurable: true });
        Object.defineProperty(globalThis, name, { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    }

    // RTCSessionDescription — dictionary-backed value object.
    globalThis.RTCSessionDescription = (function() {
        var f = function RTCSessionDescription(init) {
            if (new.target === undefined) {
                throw new TypeError("Failed to construct 'RTCSessionDescription': Please use the 'new' operator.");
            }
            init = init || {};
            this.type = init.type || '';
            this.sdp = init.sdp || '';
        };
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: 'RTCSessionDescription', configurable: true });
        Object.defineProperty(globalThis, 'RTCSessionDescription', { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    })();

    // RTCIceCandidate — dictionary-backed value object (+ toJSON).
    globalThis.RTCIceCandidate = (function() {
        var f = function RTCIceCandidate(init) {
            if (new.target === undefined) {
                throw new TypeError("Failed to construct 'RTCIceCandidate': Please use the 'new' operator.");
            }
            init = init || {};
            this.candidate = init.candidate || '';
            this.sdpMid = init.sdpMid != null ? init.sdpMid : null;
            this.sdpMLineIndex = init.sdpMLineIndex != null ? init.sdpMLineIndex : null;
            this.usernameFragment = init.usernameFragment != null ? init.usernameFragment : null;
        };
        f.prototype.toJSON = function() {
            return { candidate: this.candidate, sdpMid: this.sdpMid,
                sdpMLineIndex: this.sdpMLineIndex, usernameFragment: this.usernameFragment };
        };
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: 'RTCIceCandidate', configurable: true });
        Object.defineProperty(globalThis, 'RTCIceCandidate', { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    })();

    // RTCDataChannel — interface object; instances come from createDataChannel.
    globalThis.RTCDataChannel = (function() {
        var f = function RTCDataChannel() { throw new TypeError('Illegal constructor'); };
        var attrs = ['label', 'ordered', 'maxPacketLifeTime', 'maxRetransmits', 'protocol',
            'negotiated', 'id', 'readyState', 'bufferedAmount', 'bufferedAmountLowThreshold', 'binaryType'];
        attrs.forEach(function(name) {
            Object.defineProperty(f.prototype, name, {
                get: function() { return undefined; }, enumerable: true, configurable: true });
        });
        ['send', 'close'].forEach(function(m) {
            f.prototype[m] = function() {};
        });
        ['onopen', 'onclose', 'onmessage', 'onerror', 'onbufferedamountlow'].forEach(function(name) {
            Object.defineProperty(f.prototype, name, {
                get: function() { return null; }, set: function() {}, enumerable: true, configurable: true });
        });
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: 'RTCDataChannel', configurable: true });
        Object.defineProperty(globalThis, 'RTCDataChannel', { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    })();

    // Sender/receiver/transceiver/dtmf/transport interface objects —
    // stubs above already inherit these prototypes.
    globalThis.RTCRtpSender = (function() {
        var f = function RTCRtpSender() { throw new TypeError('Illegal constructor'); };
        f.prototype.getParameters = function() { return {}; };
        f.prototype.setParameters = function() { return Promise.reject(new DOMException('RTCRtpSender.setParameters: not supported', 'NotSupportedError')); };
        f.prototype.getStats = function() { return Promise.resolve({}); };
        f.prototype.replaceTrack = function() { return Promise.resolve(); };
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: 'RTCRtpSender', configurable: true });
        Object.defineProperty(globalThis, 'RTCRtpSender', { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    })();
    globalThis.RTCRtpReceiver = (function() {
        var f = function RTCRtpReceiver() { throw new TypeError('Illegal constructor'); };
        f.prototype.getParameters = function() { return {}; };
        f.prototype.getStats = function() { return Promise.resolve({}); };
        f.prototype.getContributingSources = function() { return []; };
        f.prototype.getSynchronizationSources = function() { return []; };
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: 'RTCRtpReceiver', configurable: true });
        Object.defineProperty(globalThis, 'RTCRtpReceiver', { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    })();
    globalThis.RTCRtpTransceiver = (function() {
        var f = function RTCRtpTransceiver() { throw new TypeError('Illegal constructor'); };
        f.prototype.stop = function() {};
        f.prototype.setCodecPreferences = function() {};
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: 'RTCRtpTransceiver', configurable: true });
        Object.defineProperty(globalThis, 'RTCRtpTransceiver', { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    })();
    globalThis.RTCDtmfSender = (function() {
        var f = function RTCDtmfSender() { throw new TypeError('Illegal constructor'); };
        f.prototype.insertDTMF = function() {};
        f.prototype.interrupt = function() {};
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: 'RTCDtmfSender', configurable: true });
        Object.defineProperty(globalThis, 'RTCDtmfSender', { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    })();
    ['RTCDtlsTransport', 'RTCIceTransport', 'RTCSctpTransport'].forEach(function(name) {
        var f = function() { throw new TypeError('Illegal constructor'); };
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: name, configurable: true });
        Object.defineProperty(globalThis, name, { value: f, writable: true, enumerable: false, configurable: true });
    });

    Object.defineProperty(globalThis, 'RTCPeerConnection', {
        value: _rtc_pc,
        writable: true,
        enumerable: false,
        configurable: true
    });
})();

// ── Chrome-exposed interface objects Lumen lacked (Ozon abt parity) ──────────
// Presence + prototype shape only; every operation rejects/throws
// NotSupportedError — same honesty rule as the RTCPeerConnection stub above.
(function() {
    function _iface(name, protoMethods, ctor) {
        var f = ctor || function() { throw new TypeError('Illegal constructor'); };
        var p = {};
        (protoMethods || []).forEach(function(m) {
            p[m] = function() {
                return Promise.reject(new DOMException(name + '.' + m + ': not supported', 'NotSupportedError'));
            };
        });
        f.prototype = Object.assign(Object.create(Object.prototype), p);
        Object.defineProperty(f.prototype, Symbol.toStringTag, { value: name, configurable: true });
        Object.defineProperty(globalThis, name, { value: f, writable: true, enumerable: false, configurable: true });
        return f;
    }
    var _notSupported = function(name, method, args) {
        return function() {
            if (!(this instanceof Object)) {}
            return Promise.reject(new DOMException(name + '.' + method + ': not supported', 'NotSupportedError'));
        };
    };

    // WebCodecs — VideoDecoder/AudioDecoder are constructible in Chrome.
    ['VideoDecoder', 'AudioDecoder'].forEach(function(n) {
        var f = function() {
            if (new.target === undefined) throw new TypeError("Failed to construct '" + n + "': Please use the 'new' operator.");
            var self = Object.create(f.prototype);
            Object.defineProperty(self, 'state', { get: function() { return 'unconfigured'; }, enumerable: true, configurable: true });
            Object.defineProperty(self, 'decodeQueueSize', { get: function() { return 0; }, enumerable: true, configurable: true });
            return self;
        };
        var proto = f.prototype;
        ['configure', 'decode', 'flush', 'reset', 'close'].forEach(function(m) {
            proto[m] = function() {
                return Promise.reject(new DOMException(n + '.' + m + ': not supported', 'NotSupportedError'));
            };
        });
        Object.defineProperty(f, 'prototype', { writable: false });
        Object.defineProperty(proto, Symbol.toStringTag, { value: n, configurable: true });
        Object.defineProperty(globalThis, n, { value: f, writable: true, enumerable: false, configurable: true });
    });

    // WebSocketStream — constructible, closed promise + close().
    var _wss = function WebSocketStream() {
        if (new.target === undefined) throw new TypeError("Failed to construct 'WebSocketStream': Please use the 'new' operator.");
        throw new DOMException('WebSocketStream: not supported', 'NotSupportedError');
    };
    Object.defineProperty(_wss.prototype, 'closed', { get: function() { return Promise.reject(new DOMException('WebSocketStream.closed: not supported', 'NotSupportedError')); }, enumerable: true, configurable: true });
    _wss.prototype.close = function() { return Promise.reject(new DOMException('WebSocketStream.close: not supported', 'NotSupportedError')); };
    _wss.prototype.getReader = function() { return Promise.reject(new DOMException('WebSocketStream.getReader: not supported', 'NotSupportedError')); };
    Object.defineProperty(_wss, 'prototype', { writable: false });
    Object.defineProperty(_wss.prototype, Symbol.toStringTag, { value: 'WebSocketStream', configurable: true });
    Object.defineProperty(globalThis, 'WebSocketStream', { value: _wss, writable: true, enumerable: false, configurable: true });

    // window.webkitRTCPeerConnection — legacy alias Chrome keeps.
    if (typeof RTCPeerConnection === 'function') {
        Object.defineProperty(globalThis, 'webkitRTCPeerConnection', {
            value: RTCPeerConnection, writable: true, enumerable: false, configurable: true });
    }

    // Navigator device-request interfaces — [SecureContext] in Chrome.
    // Each interface gets a real constructor + prototype so `typeof Serial ===
    // 'function'`, `navigator.serial instanceof Serial` and
    // `Object.prototype.toString` branding all match Chrome.
    if (typeof navigator !== 'undefined' && _lumen_secure_context !== false) {
        function _ifaceCtor(name, protoMethods, instanceProps) {
            var f = function() {
                if (new.target === undefined)
                    throw new TypeError("Failed to construct '" + name + "': Please use the 'new' operator, use 'navigator." + name.charAt(0).toLowerCase() + name.slice(1) + "' instead.");
                var o = {};
                if (instanceProps) for (var k in instanceProps) o[k] = instanceProps[k];
                return o;
            };
            f.prototype = {};
            Object.defineProperty(f.prototype, Symbol.toStringTag, { value: name, configurable: true });
            (protoMethods || []).forEach(function(m) {
                f.prototype[m] = function() {
                    return Promise.reject(new DOMException(name + '.' + m + ': not supported', 'NotSupportedError'));
                };
            });
            Object.defineProperty(f, 'prototype', { writable: false });
            Object.defineProperty(globalThis, name, { value: f, writable: true, enumerable: false, configurable: true });
            return f;
        }
        function _navInstance(ctor, extra) {
            var o = Object.create(ctor.prototype);
            if (extra) for (var k in extra) o[k] = extra[k];
            return o;
        }
        var _Serial = _ifaceCtor('Serial', ['requestPort', 'getPorts']);
        var _USB = _ifaceCtor('USB', ['getDevices', 'requestDevice']);
        var _Bluetooth = _ifaceCtor('Bluetooth', ['getAvailability', 'requestDevice', 'getDevices']);
        var _HID = _ifaceCtor('HID', ['getDevices', 'requestDevice']);
        try { Object.defineProperty(navigator, 'serial', { value: _navInstance(_Serial), enumerable: true, configurable: true }); } catch(_) {}
        try { Object.defineProperty(navigator, 'usb', { value: _navInstance(_USB), enumerable: true, configurable: true }); } catch(_) {}
        try { Object.defineProperty(navigator, 'bluetooth', { value: _navInstance(_Bluetooth), enumerable: true, configurable: true }); } catch(_) {}
        try { Object.defineProperty(navigator, 'hid', { value: _navInstance(_HID), enumerable: true, configurable: true }); } catch(_) {}

        // LaunchQueue — constructible interface + `launchQueue` singleton.
        var _LaunchQueue = _ifaceCtor('LaunchQueue', []);
        _LaunchQueue.prototype.setConsumer = function() {};
        _LaunchQueue.prototype.files = [];
        try {
            var _lq = _navInstance(_LaunchQueue);
            _lq.setConsumer = _LaunchQueue.prototype.setConsumer;
            _lq.files = [];
            Object.defineProperty(globalThis, 'launchQueue', { value: _lq, writable: false, enumerable: false, configurable: true });
        } catch(_) {}

        // DocumentPictureInPicture — `documentPictureInPicture` singleton whose
        // `window` getter rejects like Chrome without the feature flag.
        var _DPIP = _ifaceCtor('DocumentPictureInPicture', ['requestWindow']);
        try {
            var _dpp = _navInstance(_DPIP, { onenter: null });
            Object.defineProperty(_dpp, 'window', { get: function() { return null; }, enumerable: true, configurable: true });
            Object.defineProperty(globalThis, 'documentPictureInPicture', { value: _dpp, writable: false, enumerable: true, configurable: true });
        } catch(_) {}

        // webkitSpeechRecognition — prefixed constructor Chrome still ships;
        // instances fail at start() like an unsupported backend.
        var _SSR = _ifaceCtor('SpeechRecognition', []);
        _SSR.prototype.start = function() { throw new DOMException('SpeechRecognition.start: not supported', 'NotSupportedError'); };
        _SSR.prototype.stop = function() {};
        _SSR.prototype.abort = function() {};
        _SSR.prototype.addEventListener = function() {};
        _SSR.prototype.removeEventListener = function() {};
        Object.defineProperty(globalThis, 'webkitSpeechRecognition', {
            value: _SSR, writable: true, enumerable: false, configurable: true });
    }

    // Navigator.prototype parity pass (BUG-1835): members measured on
    // CfT 157.0.8103.78 that the per-API modules never installed. Defined on
    // the instance here — `finalize_navigator_interface_v8` moves them onto
    // `Navigator.prototype` with WebIDL getter semantics before sealing.
    if (typeof navigator !== 'undefined' && typeof Navigator === 'function') {
        var _secCtx = (typeof _lumen_secure_context !== 'undefined') ? _lumen_secure_context : true;
        function _navIfaceObj(name, proto) {
            var f = function() { throw new TypeError('Illegal constructor'); };
            f.prototype = proto || {};
            Object.defineProperty(f.prototype, Symbol.toStringTag, { value: name, configurable: true });
            Object.defineProperty(f, 'prototype', { writable: false });
            Object.defineProperty(globalThis, name, { value: f, writable: true, enumerable: false, configurable: true });
            return f;
        }
        function _rejectNotSupported(what) {
            return function() { return Promise.reject(new DOMException(what + ': not supported', 'NotSupportedError')); };
        }

        // ── Non-secure-context members ──
        try { navigator.vibrate = function() { return false; }; } catch(_) {}
        try { navigator.registerProtocolHandler = function() {}; } catch(_) {}
        try { navigator.unregisterProtocolHandler = function() {}; } catch(_) {}
        // navigator.webkit{Persistent,Temporary}Storage — DeprecatedStorageQuota.
        var _Dsq = _navIfaceObj('DeprecatedStorageQuota');
        _Dsq.prototype.queryUsageAndQuota = function(cb) {
            if (typeof cb === 'function') cb(0, 10 * 1024 * 1024 * 1024);
        };
        _Dsq.prototype.requestQuota = function(kind, size, cb) {
            var granted = (kind === 'persistent') ? 10 * 1024 * 1024 * 1024 : 10 * 1024 * 1024 * 1024;
            if (typeof cb === 'function') cb(granted);
        };
        try { navigator.webkitPersistentStorage = Object.create(_Dsq.prototype); } catch(_) {}
        try { navigator.webkitTemporaryStorage = Object.create(_Dsq.prototype); } catch(_) {}

        if (_secCtx) {
            // cpuPerformance — numeric UA hint (measured 3 on CfT hardware class).
            try { navigator.cpuPerformance = 3; } catch(_) {}
            // getUserMedia / webkitGetUserMedia — legacy callback aliases of
            // mediaDevices.getUserMedia.
            function _legacyGUM(constraints, success, error) {
                var p = (navigator.mediaDevices && navigator.mediaDevices.getUserMedia)
                    ? navigator.mediaDevices.getUserMedia(constraints)
                    : Promise.reject(new DOMException('getUserMedia: not supported', 'NotSupportedError'));
                p.then(function(s) { if (typeof success === 'function') success(s); },
                       function(e) { if (typeof error === 'function') error(e); });
            }
            try { navigator.getUserMedia = _legacyGUM; } catch(_) {}
            try { navigator.webkitGetUserMedia = _legacyGUM; } catch(_) {}
            try {
                navigator.getInstalledRelatedApps = function() { return Promise.resolve([]); };
            } catch(_) {}
            try {
                navigator.requestMediaKeySystemAccess = function() {
                    return Promise.reject(new DOMException('requestMediaKeySystemAccess: not supported', 'NotSupportedError'));
                };
            } catch(_) {}

            // devicePosture — foldable posture, 'continuous' on flat hardware.
            var _DP = _navIfaceObj('DevicePosture');
            Object.defineProperty(_DP.prototype, 'type', { get: function() { return 'continuous'; }, enumerable: true, configurable: true });
            _DP.prototype.onchange = null;
            try { navigator.devicePosture = Object.create(_DP.prototype); } catch(_) {}

            // scheduling — Scheduling.isInputPending().
            var _Sch = _navIfaceObj('Scheduling');
            _Sch.prototype.isInputPending = function() { return Promise.resolve(false); };
            try { navigator.scheduling = Object.create(_Sch.prototype); } catch(_) {}

            // keyboard — Keyboard.getLayoutMap/lock/unlock.
            var _Kb = _navIfaceObj('Keyboard');
            _Kb.prototype.getLayoutMap = function() { return Promise.resolve(new Map()); };
            _Kb.prototype.lock = function() { return Promise.resolve(); };
            _Kb.prototype.unlock = function() {};
            try { navigator.keyboard = Object.create(_Kb.prototype); } catch(_) {}

            // windowControlsOverlay — desktop Chrome exposes it; the event
            // handler keeps Chrome's own misspelling 'ongeometrychange'.
            var _WCO = _navIfaceObj('WindowControlsOverlay');
            Object.defineProperty(_WCO.prototype, 'visible', { get: function() { return false; }, enumerable: true, configurable: true });
            _WCO.prototype.ongeometrychange = null;
            _WCO.prototype.getTitlebarAreaRect = function() {
                if (typeof DOMRect === 'function') return new DOMRect(0, 0, 0, 0);
                return { x: 0, y: 0, width: 0, height: 0, top: 0, right: 0, bottom: 0, left: 0 };
            };
            try { navigator.windowControlsOverlay = Object.create(_WCO.prototype); } catch(_) {}

            // ink — Ink.requestPresenter (no presenter on desktop).
            var _Ink = _navIfaceObj('Ink');
            _Ink.prototype.requestPresenter = _rejectNotSupported('Ink.requestPresenter');
            try { navigator.ink = Object.create(_Ink.prototype); } catch(_) {}

            // login — Login Status API (navigator.login.setStatus).
            var _Login = _navIfaceObj('NavigatorLogin');
            _Login.prototype.setStatus = function() { return Promise.resolve(); };
            try { navigator.login = Object.create(_Login.prototype); } catch(_) {}

            // managed — Navigator Managed Configuration.
            var _Managed = _navIfaceObj('NavigatorManagedData');
            _Managed.prototype.getManifest = function() { return Promise.resolve(null); };
            _Managed.prototype.getManagedConfiguration = function() { return Promise.resolve({}); };
            try { navigator.managed = Object.create(_Managed.prototype); } catch(_) {}
        }
    }
})();
