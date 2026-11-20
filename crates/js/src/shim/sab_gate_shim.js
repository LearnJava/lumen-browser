// BUG-1829: Chrome exposes `SharedArrayBuffer` only inside a
// crossOriginIsolated context (COOP=same-origin + COEP=require-corp /
// credentialless). V8 installs the constructor unconditionally, so the shim
// removes it when the realm is not isolated. The flag arrives as
// `_LUMEN_CROSS_ORIGIN_ISOLATED` on the page (set by `install_dom` from the
// COOP/COEP response headers); worker scopes never carry it — Lumen does not
// propagate isolation into workers — so they always take the non-isolated
// branch, matching Chrome where a worker inherits isolation from its creator.
// `Atomics` stays: Chrome keeps the namespace exposed and only gates
// SAB-backed operations (`Atomics.waitAsync` is skipped by
// `tc39_proposals.rs`'s own `typeof SharedArrayBuffer` guard once the
// constructor is gone).
(function() {
    var isolated = (typeof _LUMEN_CROSS_ORIGIN_ISOLATED !== 'undefined')
        && !!_LUMEN_CROSS_ORIGIN_ISOLATED;
    if (isolated || typeof SharedArrayBuffer === 'undefined') return;
    try { delete globalThis.SharedArrayBuffer; } catch (e) {}
    // V8 intrinsics are configurable today; if a future snapshot marks the
    // property non-configurable the delete silently fails — shadow the value
    // so `typeof SharedArrayBuffer` still answers 'undefined' like Chrome's
    // absent global.
    if (typeof SharedArrayBuffer !== 'undefined') {
        try { globalThis.SharedArrayBuffer = undefined; } catch (e2) {}
    }
})();
