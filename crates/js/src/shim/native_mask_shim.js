// ── Native-code masking for Web API functions (BUG-1826) ────────────────────
// Lumen's Web APIs are implemented as JS shims over `_lumen_*` natives, so
// `Function.prototype.toString` leaks the shim source instead of reporting
// `[native code]` the way every real browser does for WebIDL operations.
// Anti-bot fingerprinting (Cloudflare managed challenge) treats that as a
// fake browser and escalates the proof-of-work into a non-terminating loop.
//
// This block runs once at the end of `install_dom`, after every interface has
// been installed and before the first page script executes. It snapshots every
// function reachable from the global object into a Set, then overrides
// `Function.prototype.toString` so a function in that set reports
// `function name() { [native code] }`. Page functions are created after this
// snapshot and keep their real source.
//
// Reachability has two passes because Web API singletons and interface
// constructors are exposed as *getters* on `window`, not data properties:
// pass 1 walks data properties (constructors, prototypes, namespace objects);
// pass 2 calls each getter found on `globalThis` and its prototype chain once
// (they return the already-installed singletons) and harvests the results, so
// `document.*`, `TextEncoder.prototype.*`, etc. are covered too.
(function () {
    if (typeof Function === 'undefined' || typeof Function.prototype === 'undefined') return;
    var originalToString = Function.prototype.toString;
    if (typeof originalToString !== 'function') return;

    var nativeFns = new Set();
    var seen = new Set();
    var queue = [globalThis];
    var budget = 1000000; // hard cap on *new* objects visited; DOM graphs are ~10^4

    function register(fn) {
        if (typeof fn === 'function') nativeFns.add(fn);
    }

    function harvest(obj) {
        if (obj === null || (typeof obj !== 'object' && typeof obj !== 'function')) return;
        if (seen.has(obj)) return;
        if (budget-- <= 0) return;
        seen.add(obj);

        var names;
        try { names = Object.getOwnPropertyNames(obj); } catch (e) { names = []; }
        for (var i = 0; i < names.length; i++) {
            var d;
            try { d = Object.getOwnPropertyDescriptor(obj, names[i]); } catch (e) { continue; }
            if (!d) continue;
            register(d.get);
            register(d.set);
            // A function is both registered as native *and* queued: its own
            // properties (`.prototype`, statics) hold more functions we must
            // reach, e.g. `TextEncoder.prototype.encode`.
            if (typeof d.value === 'function') { nativeFns.add(d.value); queue.push(d.value); }
            else if (d.value !== null && typeof d.value === 'object') queue.push(d.value);
        }

        var syms;
        try { syms = Object.getOwnPropertySymbols(obj); } catch (e) { syms = []; }
        for (var j = 0; j < syms.length; j++) {
            var ds;
            try { ds = Object.getOwnPropertyDescriptor(obj, syms[j]); } catch (e) { continue; }
            if (!ds) continue;
            register(ds.get);
            register(ds.set);
            if (typeof ds.value === 'function') { nativeFns.add(ds.value); queue.push(ds.value); }
            else if (ds.value !== null && typeof ds.value === 'object') queue.push(ds.value);
        }

        var proto;
        try { proto = Object.getPrototypeOf(obj); } catch (e) { proto = null; }
        if (proto !== null) queue.push(proto);
    }

    function drain() {
        while (queue.length > 0) harvest(queue.pop());
    }

    // Pass 1: everything reachable through data properties.
    drain();

    // Pass 2: call getters on `globalThis` and its prototype chain once and
    // harvest the singletons/constructors they return. `this` is the window so
    // Window getters resolve correctly; prototype getters that throw are
    // skipped. Only the window chain is walked — interface prototype getters
    // return primitives and are already registered as functions above.
    var root = globalThis;
    var depth = 0;
    while (root && depth < 10) {
        var gnames;
        try { gnames = Object.getOwnPropertyNames(root); } catch (e) { gnames = []; }
        for (var i = 0; i < gnames.length; i++) {
            var gd;
            try { gd = Object.getOwnPropertyDescriptor(root, gnames[i]); } catch (e) { continue; }
            if (gd && typeof gd.get === 'function') {
                var v;
                try { v = gd.get.call(globalThis); } catch (e) { continue; }
                if (v !== null && (typeof v === 'object' || typeof v === 'function')) queue.push(v);
            }
        }
        try { root = Object.getPrototypeOf(root); } catch (e) { root = null; }
        depth++;
    }
    drain();

    function nativeSource(fn) {
        var name = '';
        try { name = fn.name || ''; } catch (e) { name = ''; }
        return 'function ' + (name ? name : '') + '() { [native code] }';
    }

    var maskedToString = function toString() {
        if (nativeFns.has(this)) return nativeSource(this);
        return originalToString.call(this);
    };
    nativeFns.add(maskedToString);
    Function.prototype.toString = maskedToString;
})();
