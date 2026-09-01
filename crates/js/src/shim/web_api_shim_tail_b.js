
window.getSelection     = function() { return _lumen_selection; };
window.Range            = Range;

// ── window.getComputedStyle(element[, pseudoElt]) ────────────────────────────
// Returns a CSSStyleDeclaration-like object with resolved property values.
// Pseudo-elements are not yet supported (ignored).
// CSS Variables L1 §3 (BUG-732): a `--`-prefixed name is a custom property and
// is answered from its own snapshot — the standard-property map never carried
// them, so `getPropertyValue('--x')` used to return `''` on a page whose
// cascade had resolved `--x` perfectly well.
function _lumen_computed_property(nid, name) {
    if (nid == null) return '';
    if (name.slice(0, 2) === '--') return _lumen_get_custom_property(nid, name) || '';
    return _lumen_get_computed_style(nid, name) || '';
}
window.getComputedStyle = function(element, pseudoElt) {
    var nid = element && element.__nid__ != null ? element.__nid__ : null;
    // Cache: keyed by nid, invalidated on next call (live object semantics).
    var handler = {
        get: function(target, prop) {
            if (prop === 'getPropertyValue') {
                return function(name) {
                    return _lumen_computed_property(nid, String(name));
                };
            }
            if (prop === 'length') return 0;
            if (prop === 'item') return function() { return ''; };
            if (prop === 'cssText') return '';
            if (typeof prop === 'string' && !/^\d+$/.test(prop)) {
                // camelCase → kebab-case conversion for convenience. A custom
                // property is spelled `--x` on the object too, and survives the
                // conversion unchanged, so it routes through the same helper.
                var kebab = prop.replace(/([A-Z])/g, function(m) { return '-' + m.toLowerCase(); });
                if (nid != null) return _lumen_computed_property(nid, kebab);
            }
            return undefined;
        },
        // BUG-470: without a `has` trap, `prop in getComputedStyle(el)` falls
        // through to `Reflect.has` on the empty `{}` target and is always
        // `false` — for every property, not just the one a caller happens to
        // probe. WPT's `assert_not_inherited()` starts with exactly that
        // check, so this alone made `float`/`clear` (and anything else) read
        // as "unsupported". Mirrors `get`'s non-empty-string heuristic, same
        // one `StylePropertyMapReadOnly.prototype.has` already uses.
        has: function(target, prop) {
            if (prop === 'getPropertyValue' || prop === 'length' || prop === 'item' || prop === 'cssText') {
                return true;
            }
            if (typeof prop === 'string' && !/^\d+$/.test(prop)) {
                var kebab = prop.replace(/([A-Z])/g, function(m) { return '-' + m.toLowerCase(); });
                if (nid != null) return _lumen_computed_property(nid, kebab) !== '';
            }
            return false;
        }
    };
    // Return a Proxy if available (modern JS), otherwise a plain object with getPropertyValue.
    if (typeof Proxy !== 'undefined') {
        return new Proxy({}, handler);
    }
    // Fallback for environments without Proxy.
    return {
        getPropertyValue: function(name) {
            return _lumen_computed_property(nid, String(name));
        }
    };
};

// Restore persisted databases for this origin (no-op on first visit / when no
// backend is installed). A new JS runtime is built on every page load, so this
// is what makes IndexedDB survive a reload.
if (typeof _lumen_idb_load === 'function') {
    try {
        var _idb_saved = _lumen_idb_load();
        if (_idb_saved) {
            var _idb_restored = _idb_deserialize(_idb_saved);
            if (_idb_restored && typeof _idb_restored === 'object') _idb_databases = _idb_restored;
        }
    } catch (e) { _lumen_console_error('IDB load: ' + e); }
}

// ── Web Crypto API (W3C Web Cryptography API §3 + §14 SubtleCrypto) ──────────
// window.crypto: getRandomValues, randomUUID, subtle (SubtleCrypto).
// Algorithms: ECDSA P-256, HMAC-SHA-256/384/512, AES-GCM 128/256.
(function () {
    function getRandomValues(typedArray) {
        if (!typedArray || typeof typedArray.byteLength !== 'number')
            throw new TypeError('getRandomValues: argument must be a typed array');
        if (typedArray.byteLength > 65536)
            throw new DOMException('getRandomValues: requested too many random bytes (max 65536)', 'QuotaExceededError');
        var bytes = _lumen_get_random_bytes(typedArray.byteLength);
        var view = new Uint8Array(typedArray.buffer, typedArray.byteOffset, typedArray.byteLength);
        for (var i = 0; i < bytes.length; i++) view[i] = bytes[i];
        return typedArray;
    }

    function randomUUID() {
        // RFC 4122 §4.4 UUID version 4
        var b = _lumen_get_random_bytes(16);
        b[6] = (b[6] & 0x0f) | 0x40;  // version 4
        b[8] = (b[8] & 0x3f) | 0x80;  // variant 10xx
        var h = b.map(function(x) { return ('0' + x.toString(16)).slice(-2); });
        return h.slice(0, 4).join('') + '-' + h.slice(4, 6).join('') + '-' +
               h.slice(6, 8).join('') + '-' + h.slice(8, 10).join('') + '-' +
               h.slice(10).join('');
    }

    // Opaque CryptoKey object — wraps a Rust-side key id.
    function CryptoKey(id, info) {
        this.__ckid   = id;
        this.type       = info.type;
        this.algorithm  = info.algorithm;
        this.extractable = info.extractable;
        this.usages     = info.usages;
    }

    function _make_crypto_key(id) {
        var infoJson = _lumen_subtle_key_info(id);
        if (!infoJson) throw new DOMException('Internal: key not found', 'OperationError');
        var info = JSON.parse(infoJson);
        return new CryptoKey(id, info);
    }

    function _to_bytes(data) {
        if (data instanceof ArrayBuffer) return Array.from(new Uint8Array(data));
        if (ArrayBuffer.isView(data))    return Array.from(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
        throw new TypeError('SubtleCrypto: data must be a BufferSource');
    }

    function _alg_json(algorithm) {
        if (typeof algorithm === 'string') return JSON.stringify({ name: algorithm });
        return JSON.stringify(algorithm);
    }

    function _usages_json(usages) {
        return JSON.stringify(Array.isArray(usages) ? usages : []);
    }

    function _dom_err(result) {
        // result starts with err: prefix
        var msg = result.slice(4);
        return new DOMException(msg, msg);
    }

    var subtle = {
        // ── digest ───────────────────────────────────────────────────────────
        digest: function (algorithm, data) {
            var algo = (algorithm && typeof algorithm === 'object' && algorithm.name)
                     ? algorithm.name : String(algorithm);
            return new Promise(function (resolve, reject) {
                try {
                    var inputBytes = _to_bytes(data);
                    var result = _lumen_sha_digest(algo, inputBytes);
                    if (!result || result.length === 0) {
                        reject(new DOMException(
                            'SubtleCrypto.digest: unsupported algorithm: ' + algo,
                            'NotSupportedError'));
                        return;
                    }
                    resolve(new Uint8Array(result).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── generateKey ──────────────────────────────────────────────────────
        generateKey: function (algorithm, extractable, keyUsages) {
            return new Promise(function (resolve, reject) {
                try {
                    var algJson = _alg_json(algorithm);
                    var usagesJson = _usages_json(keyUsages);
                    var result = _lumen_subtle_generate_key(algJson, !!extractable, usagesJson);
                    if (result.startsWith('err:')) { reject(_dom_err(result)); return; }
                    // ECDSA key pair: pub_id comma priv_id
                    if (result.indexOf(',') !== -1) {
                        var parts = result.split(',');
                        resolve({
                            publicKey:  _make_crypto_key(parseInt(parts[0], 10)),
                            privateKey: _make_crypto_key(parseInt(parts[1], 10))
                        });
                    } else {
                        resolve(_make_crypto_key(parseInt(result, 10)));
                    }
                } catch (e) { reject(e); }
            });
        },

        // ── importKey ────────────────────────────────────────────────────────
        importKey: function (format, keyData, algorithm, extractable, keyUsages) {
            return new Promise(function (resolve, reject) {
                try {
                    var algJson = _alg_json(algorithm);
                    var usagesJson = _usages_json(keyUsages);
                    var bytes;
                    if (format === 'jwk') {
                        // keyData is a JWK object — stringify it to UTF-8 bytes
                        bytes = Array.from(new TextEncoder().encode(JSON.stringify(keyData)));
                    } else {
                        bytes = _to_bytes(keyData instanceof ArrayBuffer ? keyData
                            : (ArrayBuffer.isView(keyData) ? keyData : new Uint8Array(0)));
                    }
                    var result = _lumen_subtle_import_key(format, bytes, algJson, !!extractable, usagesJson);
                    if (result.startsWith('err:')) { reject(_dom_err(result)); return; }
                    resolve(_make_crypto_key(parseInt(result, 10)));
                } catch (e) { reject(e); }
            });
        },

        // ── exportKey ────────────────────────────────────────────────────────
        exportKey: function (format, key) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('exportKey: argument is not a CryptoKey')); return;
                    }
                    var result = _lumen_subtle_export_key_or_err(format, key.__ckid);
                    if (result.startsWith('err:')) { reject(_dom_err(result)); return; }
                    if (result.startsWith('hex:')) {
                        // Raw bytes in hex form
                        var hex = result.slice(4);
                        var buf = new Uint8Array(hex.length / 2);
                        for (var i = 0; i < buf.length; i++)
                            buf[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
                        resolve(format === 'jwk' ? JSON.parse(new TextDecoder().decode(buf)) : buf.buffer);
                    } else {
                        // ok:... prefix for JWK JSON
                        var json = result.slice(3);
                        resolve(format === 'jwk' ? JSON.parse(json) : new TextEncoder().encode(json).buffer);
                    }
                } catch (e) { reject(e); }
            });
        },

        // ── sign ─────────────────────────────────────────────────────────────
        sign: function (algorithm, key, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('sign: argument is not a CryptoKey')); return;
                    }
                    var algJson = _alg_json(algorithm);
                    var dataBytes = _to_bytes(data);
                    var sig = _lumen_subtle_sign(algJson, key.__ckid, dataBytes);
                    if (!sig || sig.length === 0) {
                        reject(new DOMException('sign: operation failed', 'OperationError')); return;
                    }
                    resolve(new Uint8Array(sig).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── verify ───────────────────────────────────────────────────────────
        verify: function (algorithm, key, signature, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('verify: argument is not a CryptoKey')); return;
                    }
                    var algJson = _alg_json(algorithm);
                    var sigBytes  = _to_bytes(signature);
                    var dataBytes = _to_bytes(data);
                    var ok = _lumen_subtle_verify(algJson, key.__ckid, sigBytes, dataBytes);
                    resolve(!!ok);
                } catch (e) { reject(e); }
            });
        },

        // ── encrypt (RSA-OAEP / AES-GCM / AES-CBC / AES-CTR) ────────────────
        encrypt: function (algorithm, key, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('encrypt: argument is not a CryptoKey')); return;
                    }
                    var algName = (algorithm && algorithm.name) ? algorithm.name.toUpperCase() : '';
                    var pt = _to_bytes(data);
                    var ct;
                    if (algName === 'RSA-OAEP') {
                        var label = algorithm.label ? _to_bytes(algorithm.label) : [];
                        ct = _lumen_subtle_rsa_oaep_encrypt(key.__ckid, label, pt);
                    } else if (algName === 'AES-CBC') {
                        var iv = _to_bytes(algorithm.iv || new Uint8Array(16));
                        ct = _lumen_subtle_aes_cbc_encrypt(key.__ckid, iv, pt);
                    } else if (algName === 'AES-CTR') {
                        var counter = _to_bytes(algorithm.counter || new Uint8Array(16));
                        var len = (algorithm.length !== undefined) ? algorithm.length : 64;
                        ct = _lumen_subtle_aes_ctr_crypt(key.__ckid, counter, len, pt);
                    } else {
                        // AES-GCM (default)
                        var iv  = _to_bytes(algorithm.iv || new Uint8Array(12));
                        var aad = algorithm.additionalData ? _to_bytes(algorithm.additionalData) : [];
                        ct = _lumen_subtle_encrypt(key.__ckid, iv, aad, pt);
                    }
                    if (!ct || ct.length === 0) {
                        reject(new DOMException('encrypt: operation failed', 'OperationError')); return;
                    }
                    resolve(new Uint8Array(ct).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── decrypt (RSA-OAEP / AES-GCM / AES-CBC / AES-CTR) ────────────────
        decrypt: function (algorithm, key, data) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('decrypt: argument is not a CryptoKey')); return;
                    }
                    var algName = (algorithm && algorithm.name) ? algorithm.name.toUpperCase() : '';
                    var ct = _to_bytes(data);
                    var pt;
                    if (algName === 'RSA-OAEP') {
                        var label = algorithm.label ? _to_bytes(algorithm.label) : [];
                        pt = _lumen_subtle_rsa_oaep_decrypt(key.__ckid, label, ct);
                    } else if (algName === 'AES-CBC') {
                        var iv = _to_bytes(algorithm.iv || new Uint8Array(16));
                        pt = _lumen_subtle_aes_cbc_decrypt(key.__ckid, iv, ct);
                    } else if (algName === 'AES-CTR') {
                        var counter = _to_bytes(algorithm.counter || new Uint8Array(16));
                        var len = (algorithm.length !== undefined) ? algorithm.length : 64;
                        pt = _lumen_subtle_aes_ctr_crypt(key.__ckid, counter, len, ct);
                    } else {
                        // AES-GCM (default)
                        var iv  = _to_bytes(algorithm.iv || new Uint8Array(12));
                        var aad = algorithm.additionalData ? _to_bytes(algorithm.additionalData) : [];
                        pt = _lumen_subtle_decrypt(key.__ckid, iv, aad, ct);
                    }
                    if (!pt || pt.length === 0) {
                        reject(new DOMException('decrypt: operation failed', 'OperationError')); return;
                    }
                    resolve(new Uint8Array(pt).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── deriveBits (PBKDF2 / HKDF / ECDH) ───────────────────────────────
        deriveBits: function (algorithm, key, length) {
            return new Promise(function (resolve, reject) {
                try {
                    if (!(key instanceof CryptoKey)) {
                        reject(new TypeError('deriveBits: argument is not a CryptoKey')); return;
                    }
                    var alg = (typeof algorithm === 'string') ? { name: algorithm } : algorithm;
                    var algName = (alg.name || '').toUpperCase();
                    var bits;
                    if (algName === 'ECDH') {
                        // ECDH: algorithm.public is the peer's CryptoKey
                        var peerKeyId = (alg.public instanceof CryptoKey) ? alg.public.__ckid : 0;
                        var algFull = JSON.stringify({ name: alg.name, publicKeyId: peerKeyId });
                        bits = _lumen_subtle_derive_bits(algFull, key.__ckid, length || 256);
                    } else {
                        var hashName = (alg.hash && alg.hash.name) ? alg.hash.name : (alg.hash || 'SHA-256');
                        var salt = alg.salt ? Array.from(_to_bytes(alg.salt)) : [];
                        var info = alg.info ? Array.from(_to_bytes(alg.info)) : [];
                        var algFull = JSON.stringify({
                            name: alg.name,
                            hash: hashName,
                            salt: salt,
                            info: info,
                            iterations: alg.iterations || 100000
                        });
                        bits = _lumen_subtle_derive_bits(algFull, key.__ckid, length || 256);
                    }
                    if (!bits || bits.length === 0) {
                        reject(new DOMException('deriveBits: operation failed', 'OperationError')); return;
                    }
                    resolve(new Uint8Array(bits).buffer);
                } catch (e) { reject(e); }
            });
        },

        // ── deriveKey (PBKDF2 / HKDF → any symmetric key) ────────────────────
        deriveKey: function (algorithm, baseKey, derivedKeyAlgorithm, extractable, keyUsages) {
            var self = this;
            return new Promise(function (resolve, reject) {
                try {
                    var dkaName = (derivedKeyAlgorithm && derivedKeyAlgorithm.name)
                        ? derivedKeyAlgorithm.name.toUpperCase() : '';
                    // Determine how many bits are needed for the target key type.
                    var length = derivedKeyAlgorithm.length
                        || ((dkaName === 'AES-GCM' || dkaName === 'AES-CBC' || dkaName === 'AES-CTR')
                            ? 256 : 256);
                    self.deriveBits(algorithm, baseKey, length)
                        .then(function (rawBits) {
                            return self.importKey('raw', rawBits, derivedKeyAlgorithm, extractable, keyUsages);
                        })
                        .then(resolve)
                        .catch(reject);
                } catch (e) { reject(e); }
            });
        },

        // ── wrapKey / unwrapKey — stubs ───────────────────────────────────────
        wrapKey: function() {
            return Promise.reject(new DOMException('wrapKey: not implemented', 'NotSupportedError'));
        },
        unwrapKey: function() {
            return Promise.reject(new DOMException('unwrapKey: not implemented', 'NotSupportedError'));
        }
    };

    window.CryptoKey = CryptoKey;
    window.crypto = { getRandomValues: getRandomValues, randomUUID: randomUUID, subtle: subtle };
    window.Crypto = function Crypto() {};
})();

// ── structuredClone (HTML LS §2.7 — StructuredSerialize/Deserialize) ─────────
// Handles: primitives (incl. BigInt), plain objects, arrays, Date, RegExp,
// Map, Set, Boolean/Number/String wrapper objects, ArrayBuffer, typed arrays
// (Int8..Float64, BigInt64/BigUint64), DataView. Preserves shared references
// and cycles via a memory map (same original → same clone). Throws a
// DataCloneError DOMException for non-serializable values (functions, symbols).
// Not handled: the `transfer` option (transferables are copied, not detached),
// Blob/File/ImageData/Error and other platform objects.
// Extension point for `[Serializable]` platform interfaces (HTML LS §2.7.2).
// A platform object is not a plain object: cloned as one it loses its class and
// its internal slots, which for a File System Access handle means the clone is
// an inert `{}` instead of a handle (BUG-374 point 9). Interfaces that declare
// `[Serializable]` register a (test, clone) pair from their own shim; the list
// itself stays closed over, so a page can add a cloner for its own objects but
// cannot read or drop anyone else's.
(function() {
    var CLONERS = [];
    Object.defineProperty(window, '__lumen_platform_cloners', {
        value: Object.freeze({
            register: function(test, clone) { CLONERS.push([test, clone]); },
            find: function(v) {
                for (var i = 0; i < CLONERS.length; i++) {
                    try { if (CLONERS[i][0](v)) return CLONERS[i][1]; } catch (e) {}
                }
                return null;
            }
        }),
        enumerable: false, writable: false, configurable: false
    });
})();

function structuredClone(val) {
    // memory: original object → its clone, so shared refs and cycles round-trip.
    var memory = new Map();
    function clone(v) {
        if (v === null) return null;
        var t = typeof v;
        if (t === 'undefined' || t === 'boolean' || t === 'number' ||
            t === 'string' || t === 'bigint') {
            return v;
        }
        if (t === 'symbol' || t === 'function') {
            throw new DOMException(
                'structuredClone: value could not be cloned', 'DataCloneError');
        }
        // t === 'object' from here on.
        if (memory.has(v)) return memory.get(v);
        // Value-immutable objects: no interior references → no cycle to register.
        if (v instanceof Date) return new Date(v.getTime());
        if (v instanceof RegExp) return new RegExp(v.source, v.flags);
        if (v instanceof Boolean) return new Boolean(v.valueOf());
        if (v instanceof Number) return new Number(v.valueOf());
        if (v instanceof String) return new String(v.valueOf());
        // Binary data: copy the backing buffer, then re-view it.
        if (v instanceof ArrayBuffer) {
            var abClone = v.slice(0);
            memory.set(v, abClone);
            return abClone;
        }
        if (typeof SharedArrayBuffer !== 'undefined' && v instanceof SharedArrayBuffer) {
            // A SharedArrayBuffer is shared by reference, never copied.
            memory.set(v, v);
            return v;
        }
        if (ArrayBuffer.isView(v)) {
            var srcBuf = v.buffer;
            var bufClone = memory.get(srcBuf);
            if (bufClone === undefined) {
                bufClone = srcBuf.slice(0);
                memory.set(srcBuf, bufClone);
            }
            var viewClone = (v instanceof DataView)
                ? new DataView(bufClone, v.byteOffset, v.byteLength)
                : new v.constructor(bufClone, v.byteOffset, v.length);
            memory.set(v, viewClone);
            return viewClone;
        }
        if (v instanceof Map) {
            var m = new Map();
            memory.set(v, m);
            v.forEach(function(entryVal, entryKey) {
                m.set(clone(entryKey), clone(entryVal));
            });
            return m;
        }
        if (v instanceof Set) {
            var s = new Set();
            memory.set(v, s);
            v.forEach(function(entryVal) { s.add(clone(entryVal)); });
            return s;
        }
        if (Array.isArray(v)) {
            var arr = new Array(v.length);
            memory.set(v, arr);
            for (var i = 0; i < v.length; i++) arr[i] = clone(v[i]);
            return arr;
        }
        // A `[Serializable]` platform object serializes through its own shim.
        var platformClone = window.__lumen_platform_cloners.find(v);
        if (platformClone) {
            var pc = platformClone(v);
            memory.set(v, pc);
            return pc;
        }
        // Plain object: own enumerable string-keyed properties only (symbol keys
        // are dropped, matching the spec's serialization of ordinary objects).
        var out = {};
        memory.set(v, out);
        var keys = Object.keys(v);
        for (var k = 0; k < keys.length; k++) out[keys[k]] = clone(v[keys[k]]);
        return out;
    }
    return clone(val);
}
window.structuredClone = structuredClone;

// ── Page lifecycle driver functions (called from Rust via QuickJsRuntime) ─────

// Drive document.readyState forward: 'loading' → 'interactive' → 'complete'.
// Idempotent — state only advances forward.
// Called by Rust: after HTML parse → 'interactive'; after all resources loaded → 'complete'.
function _lumen_apply_ready_state(state) {
    if (state === 'interactive' && _doc_ready_state !== 'loading') return;
    if (state === 'complete' && _doc_ready_state === 'complete') return;
    _doc_ready_state = state;
    // readystatechange on document
    var rsEv = new Event('readystatechange', { bubbles: false, cancelable: false });
    document.dispatchEvent(rsEv);
    if (state === 'interactive') {
        // BUG-826: the parser's `<link rel=preload|modulepreload|prefetch>`
        // elements start their fetch here — parsing is done, so the document
        // holds every hint the markup carries, and a hint appended by a head
        // script has already run through the insertion hook (the per-node
        // guard keeps it from running twice).
        _lumen_link_hints_scan();
        // BUG-838: the parser's `<script src="">` elements report their `error`
        // from here for the same reason — the markup's scripts never pass
        // through the insertion hook, and the shell's collector drops an empty
        // src without a word. The per-node guard keeps an element a head script
        // already appended from reporting twice.
        _lumen_script_empty_src_scan();
        // BUG-804: the parser's `<style>` blocks report `load` (or `error`, if
        // an `@import` inside cannot be obtained) from here — same reason
        // again, and the per-node flag keeps a `<style>` a head script built
        // from reporting its first update twice.
        _lumen_style_blocks_scan();
        // BUG-804: the parser's `<track>` elements start the §4.8.11.1 track
        // processing model from here — same reason once more. The model itself
        // lives in the media shim (`video_bindings.rs`), which is its own
        // `rt.eval` and is absent from the DOM-less runtimes, hence the guard.
        if (typeof _lumen_track_elements_scan === 'function') _lumen_track_elements_scan();
        // BUG-851: a `<details open>` the parser wrote owes a `toggle` event for
        // the same reason — markup never passes through the attribute-write hook
        // that the §4.11.1 change steps hang on. The per-node record keeps an
        // element a script has already moved from reporting a second time.
        _lumen_details_open_scan();
        // DOMContentLoaded fires on document (bubbles) then window
        var dcl = new Event('DOMContentLoaded', { bubbles: true, cancelable: false });
        document.dispatchEvent(dcl);
        var winArr = _domcontentloaded_win_listeners.slice();
        for (var i = 0; i < winArr.length; i++) {
            try { winArr[i].call(window, dcl); } catch(e) { _lumen_report_exception(e); }
        }
        // HTML LS §6.6.6 «flush autofocus candidates» (BUG-381): once parsing is
        // done, focus the first `[autofocus]` element in the document — unless a
        // DOMContentLoaded handler already moved focus itself.
        if (_lumen_last_focused_nid === -1) {
            var afNid = _lumen_find_autofocus_in(_lumen_root_nid);
            if (afNid !== -1 && _lumen_is_focusable(afNid)) {
                var afEl = _lumen_make_element(afNid);
                if (afEl && typeof afEl.focus === 'function') { try { afEl.focus(); } catch(e) {} }
            }
        }
    } else if (state === 'complete') {
        // load fires on window (does not bubble)
        var loadEv = new Event('load', { bubbles: false, cancelable: false });
        var loadArr = _load_listeners.slice();
        for (var j = 0; j < loadArr.length; j++) {
            try { loadArr[j].call(window, loadEv); } catch(e) { _lumen_report_exception(e); }
        }
        if (typeof window.onload === 'function') {
            try { window.onload.call(window, loadEv); } catch(e) { _lumen_report_exception(e); }
        }
    }
}

// Drive document.visibilityState.  Called from Rust on window focus/blur.
// hidden=true → 'hidden'; hidden=false → 'visible'.
// Fires visibilitychange on document + window listeners if state changed.
function _lumen_apply_visibility(hidden) {
    if (_doc_hidden === hidden) return;
    _doc_hidden = hidden;
    _doc_visibility_state = hidden ? 'hidden' : 'visible';
    var ev = new Event('visibilitychange', { bubbles: true, cancelable: false });
    document.dispatchEvent(ev);
    var vcArr = _visibilitychange_listeners.slice();
    for (var i = 0; i < vcArr.length; i++) {
        try { vcArr[i].call(window, ev); } catch(e) { _lumen_report_exception(e); }
    }
}

window._lumen_apply_ready_state = _lumen_apply_ready_state;
window._lumen_apply_visibility  = _lumen_apply_visibility;

// ── <dialog> modal stack (HTML5 §4.11.7) ─────────────────────────────────────
// Tracks nids of dialogs opened via showModal(), in open order.
// Maintained by _lumen_make_element's showModal/close methods (see below).
var _lumen_modal_dialog_nids = [];

// nid of the element that had keyboard focus immediately before the most
// recent showModal() call (-1 = none). Used to restore focus on close.
var _lumen_last_focused_nid = -1;

// Per-dialog saved focus nid: restored when that dialog closes.
var _lumen_dialog_prev_focus = {};

// DFS search for the first descendant of `container_nid` that has an
// `autofocus` attribute. Returns its nid, or -1 if none found.
// Presence is tested through `_lumen_has_attr`, not `!== undefined`: on the V8
// bindings a missing attribute reads as `null` (BUG-442), which made this
// return the container's first child on every call.
function _lumen_find_autofocus_in(container_nid) {
    var queue = _lumen_get_children(container_nid).slice();
    while (queue.length > 0) {
        var cur = queue.shift();
        if (_lumen_has_attr(cur, 'autofocus')) return cur;
        var ch = _lumen_get_children(cur);
        for (var i = 0; i < ch.length; i++) queue.push(ch[i]);
    }
    return -1;
}

// ── Focus management (HTML LS §6.6) ──────────────────────────────────────────
// BUG-381. The shell owns the real focus state (`Shell.focused_node` — it feeds
// `:focus` matching, keyboard/IME routing and the platform a11y bridge) and
// reports every change here through `_lumen_focus_update`. The page moves focus
// the other way: `element.focus()` updates this side synchronously (the spec
// requires `document.activeElement` to be current on the very next statement)
// and queues `_lumen_request_focus` for the shell to apply on its next pump.
// Both directions funnel through `_lumen_focus_update`, so the event sequence is
// emitted exactly once and `_lumen_last_focused_nid` keeps a single writer.

// Tags whose `disabled` content attribute takes them out of the focus order.
var _LUMEN_DISABLEABLE_TAGS = {
    INPUT: 1, SELECT: 1, TEXTAREA: 1, BUTTON: 1, OPTGROUP: 1, OPTION: 1, FIELDSET: 1,
};

// Tags that are focusable without a `tabindex` attribute (HTML LS §6.6.1).
// `A`/`AREA` (need `href`), `INPUT` (any type but `hidden`) and `AUDIO`/`VIDEO`
// (need `controls`) are conditional, so they are handled separately below.
var _LUMEN_FOCUSABLE_TAGS = {
    SELECT: 1, TEXTAREA: 1, BUTTON: 1, IFRAME: 1, EMBED: 1, OBJECT: 1, SUMMARY: 1,
};

// HTML LS §2.4.4.1 «rules for parsing integers»: skip ASCII whitespace, take an
// optional sign, require at least one ASCII digit, then «collect a sequence of
// code points that are ASCII digits» and STOP. Returns null on error (absent,
// empty, no digit where one is required).
//
// BUG-452: the trailing tail is ignored, not rejected. This used to bail out on
// the first non-digit («deliberately stricter than parseInt, which would accept
// '12px'»), which is stricter than the spec rather than than `parseInt`: step 8
// collects digits and returns, so `'100em'`, `'100.999'`, `'100#!?'` and
// `'0x100'` are 100/100/100/**0**, not errors. Three WPT expectations turn on
// exactly that (`2d.canvas.host.size.attributes.parse.{hex,trailingjunk,em}`),
// and `tabindex='3zzz'` answered −1 instead of 3 through the same line.
// `trim()` is likewise not the spec's skip: it also eats U+00A0 and the rest of
// Unicode whitespace, where §2.4.4.1 skips ASCII whitespace only.
function _lumen_parse_integer(v) {
    if (v === null || v === undefined) return null;
    var s = String(v);
    var i = 0;
    // ASCII whitespace: TAB, LF, FF, CR, SPACE (Infra §4.6).
    while (i < s.length) {
        var ws = s.charCodeAt(i);
        if (ws !== 9 && ws !== 10 && ws !== 12 && ws !== 13 && ws !== 32) break;
        i++;
    }
    if (i >= s.length) return null;
    var sign = 1;
    if (s.charAt(i) === '-') { sign = -1; i++; }
    else if (s.charAt(i) === '+') { i++; }
    if (i >= s.length) return null;
    var c = s.charCodeAt(i);
    if (c < 48 || c > 57) return null;
    var n = 0;
    for (; i < s.length; i++) {
        c = s.charCodeAt(i);
        if (c < 48 || c > 57) break;
        n = n * 10 + (c - 48);
    }
    return sign * n;
}

// Reflection range guards (HTML LS §2.6.2). A reflected `long` answers the
// default outside −2147483648…2147483647, a reflected `unsigned long` outside
// 0…2147483647 — the parse itself has no upper bound, so without these
// `<img width="2147483648">` read back verbatim as 2147483648.
var _LUMEN_LONG_MAX = 2147483647;
var _LUMEN_LONG_MIN = -2147483648;

// Nearest inclusive ancestor of `nid` that is an element, or -1. The shell
// tracks focus by layout box and a box's node can be a text node, while the
// spec-level focus surface only ever exposes elements.
function _lumen_nearest_element_nid(nid) {
    var cur = nid;
    for (var guard = 0; guard < 512; guard++) {
        if (cur === null || cur === undefined || cur === -1) return -1;
        if (!_lumen_is_text_node(cur) && !_lumen_is_comment_node(cur)) return cur;
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    return -1;
}

// HTML LS §6.6.1 — can `nid` become a focusable area? `<body>`/`<html>` answer
// yes because scripts legitimately focus them to drop focus back to the
// viewport, even though they carry no tab index of their own.
function _lumen_is_focusable(nid) {
    if (nid === null || nid === undefined || nid === -1) return false;
    if (_lumen_is_text_node(nid) || _lumen_is_comment_node(nid)) return false;
    // HTML LS §6.7: nothing inside an inert subtree is focusable.
    var anc = nid;
    for (var guard = 0; guard < 512 && anc !== null && anc !== undefined; guard++) {
        if (_lumen_has_attr(anc, 'inert')) return false;
        anc = _lumen_u2n(_lumen_get_parent(anc));
    }
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    if (_LUMEN_DISABLEABLE_TAGS[tag] === 1 && _lumen_has_attr(nid, 'disabled')) {
        return false;
    }
    // An explicit, parseable `tabindex` makes any element focusable.
    if (_lumen_parse_integer(_lumen_u2n(_lumen_get_attr(nid, 'tabindex'))) !== null) return true;
    var ce = _lumen_u2n(_lumen_get_attr(nid, 'contenteditable'));
    if (ce !== null && String(ce).toLowerCase() !== 'false') return true;
    if (tag === 'INPUT') {
        return (_lumen_u2n(_lumen_get_attr(nid, 'type')) || '').toLowerCase() !== 'hidden';
    }
    if (tag === 'A' || tag === 'AREA') return _lumen_has_attr(nid, 'href');
    if (tag === 'AUDIO' || tag === 'VIDEO') return _lumen_has_attr(nid, 'controls');
    if (tag === 'BODY' || tag === 'HTML') return true;
    return _LUMEN_FOCUSABLE_TAGS[tag] === 1;
}

// Dispatch one focus-family event. `focus`/`blur` do not bubble and must not
// reach document-level listeners either, which is why this cannot reuse
// `_lumen_dispatch_rich` (that one runs the document listeners even for a
// non-bubbling event). `on<type>` properties are honoured at every hop, so
// `el.onfocus = fn` works next to `addEventListener('focus', fn)`.
function _lumen_dispatch_focus_event(nid, type, bubbles, related) {
    var ev = new FocusEvent(type, {
        bubbles: bubbles, cancelable: false, isTrusted: true, relatedTarget: related,
    });
    ev.target = _lumen_make_element(nid);
    var cur = nid;
    for (var guard = 0; guard < 512 && cur !== null && cur !== undefined; guard++) {
        var el = _lumen_make_element(cur);
        ev.currentTarget = el;
        var arr = _lumen_listeners[String(cur) + ':' + type];
        if (arr) {
            var copy = arr.slice();
            for (var i = 0; i < copy.length; i++) {
                if (ev.cancelBubble) break;
                try { copy[i].call(el, ev); } catch (e) { _lumen_report_exception(e); }
                if (ev._stopImmediate) break;
            }
        }
        if (el && typeof el['on' + type] === 'function') {
            try { el['on' + type].call(el, ev); } catch (e) { _lumen_report_exception(e); }
        }
        if (!bubbles || ev.cancelBubble || ev._stopImmediate) break;
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    if (bubbles && !ev.cancelBubble && !ev._stopImmediate) {
        var darr = _lumen_listeners[String(_LUMEN_DOC_LISTENER_NID) + ':' + type];
        if (darr) {
            var dcopy = darr.slice();
            ev.currentTarget = document;
            for (var j = 0; j < dcopy.length; j++) {
                if (ev.cancelBubble) break;
                try { dcopy[j].call(document, ev); } catch (e) { _lumen_report_exception(e); }
                if (ev._stopImmediate) break;
            }
        }
    }
    ev.currentTarget = null;
}

// HTML LS §6.6.4/§6.6.5 — record a focus change and fire the event sequence
// `blur` (old) → `focusout` (old) → `focus` (new) → `focusin` (new).
// `newNid` is a node id, or -1/null for «nothing focused». Idempotent: focusing
// the already-focused node fires nothing, which is exactly what stops the
// shell's echo of a page-initiated `focus()` from dispatching a second round.
function _lumen_focus_update(newNid) {
    if (newNid === null || newNid === undefined) newNid = -1;
    newNid = (newNid === -1) ? -1 : _lumen_nearest_element_nid(newNid);
    var oldNid = _lumen_last_focused_nid;
    if (oldNid === null || oldNid === undefined) oldNid = -1;
    if (oldNid === newNid) return;
    _lumen_last_focused_nid = newNid;
    var oldEl = (oldNid !== -1) ? _lumen_make_element(oldNid) : null;
    var newEl = (newNid !== -1) ? _lumen_make_element(newNid) : null;
    if (oldEl !== null) {
        _lumen_dispatch_focus_event(oldNid, 'blur', false, newEl);
        _lumen_dispatch_focus_event(oldNid, 'focusout', true, newEl);
    }
    if (newEl !== null) {
        _lumen_dispatch_focus_event(newNid, 'focus', false, oldEl);
        _lumen_dispatch_focus_event(newNid, 'focusin', true, oldEl);
    }
}
window._lumen_focus_update = _lumen_focus_update;

// HTML LS §6.6.3 — `HTMLElement.focus(options)` / `HTMLElement.blur()`. The
// shell is notified through the very `_lumen_request_focus`/`_lumen_request_blur`
// pair `<dialog>.showModal()` already used, so `:focus`, keyboard routing and
// the a11y bridge follow on its next pump.
HTMLElement.prototype.focus = function(options) {
    var nid = this.__nid__;
    if (nid === null || nid === undefined) return;
    if (!_lumen_is_focusable(nid)) return;
    _lumen_request_focus(nid);
    _lumen_focus_update(nid);
    // HTML LS §6.6.3 «scroll into view» step, unless the caller opted out.
    if (!(options && options.preventScroll) && typeof this.scrollIntoView === 'function') {
        try { this.scrollIntoView(); } catch (e) {}
    }
};
HTMLElement.prototype.blur = function() {
    var nid = this.__nid__;
    if (nid === null || nid === undefined) return;
    // Blurring an element that is not focused is a no-op (HTML LS §6.6.3).
    if (_lumen_last_focused_nid !== _lumen_nearest_element_nid(nid)) return;
    _lumen_request_blur();
    _lumen_focus_update(-1);
};

// HTML LS §7.2.2 — `window.focus()` / `window.blur()`. Lumen never lets a page
// raise or lower its own OS window (that stays a user action), so both are
// no-ops; they exist because feature detection and focus-trap code call them
// unconditionally and used to die on `is not a function`.
window.focus = function() {};
window.blur  = function() {};

// ── Declarative IDL attribute reflection (HTML LS §2.6.1, BUG-383) ───────────
// Until this block the live-element factory hand-listed the few reflected
// attributes some earlier fix happened to need (`value`, `name`, `type`,
// `checked`, `src`) — so `a.href`, `input.disabled`, `textarea.rows` and forty
// more were plain `undefined`, and every new one meant editing the factory
// again. Reflection is a mechanical mapping «IDL name ↔ content attribute ↔
// type», so it is written here once as a table and installed through one
// generic accessor pair per kind, onto the *interface prototypes* rather than
// onto every instance: adding an attribute is now a single table row, and the
// properties cost nothing per element (which is also why they no longer show up
// in `Object.getOwnPropertyNames(el)` — BUG-367's complaint).
//
// Kinds, per HTML LS §2.6.1 «reflecting content attributes in IDL attributes»:
//   string — DOMString; absent → ''
//   bool   — boolean; presence of the attribute
//   long   — long; absent or unparseable → `extra` (the default)
//   ulong  — unsigned long; absent, negative or unparseable → `extra`
//   url    — DOMString reflecting a URL; the getter returns the value resolved
//            against the document base URL, the setter stores it verbatim
//   enum   — limited to known values; `extra` = { def: <default>, keys: [...] }.
//            The spec distinguishes a missing-value default from an
//            invalid-value default; the two coincide for every attribute in the
//            table below, so one `def` covers both.

// HTML LS §4.2.3 «document base URL»: the `href` of the first <base> element
// resolved against the document URL, falling back to the document URL itself.
// Also the value behind the script-visible `Node.baseURI` (BUG-377): the
// accessor on `Node.prototype` (and the own copies on the four prototype-less
// node literals) delegate here, so the two can never disagree.
function _lumen_document_base_url() {
    var docUrl = '';
    try {
        if (typeof location !== 'undefined' && location.href) docUrl = String(location.href);
    } catch (e) {}
    var baseEl = null;
    try { baseEl = document.querySelector('base'); } catch (e) {}
    if (baseEl) {
        var h = baseEl.getAttribute('href');
        if (h !== null && h !== undefined && String(h) !== '') {
            return _url_resolve(String(h), docUrl);
        }
    }
    return docUrl;
}

// «Reflect a URL» steps: an absent or empty attribute reflects as '' (NOT as the
// document URL), anything else is resolved against the document base URL.
function _lumen_reflect_url(nid, attr) {
    var v = _lumen_u2n(_lumen_get_attr(nid, attr));
    if (v === null || String(v) === '') return '';
    return _url_resolve(String(v), _lumen_document_base_url());
}

// The node id behind a reflection receiver, or -1 when the accessor was called
// on something that is not a live element (`HTMLInputElement.prototype.disabled`
// read directly off the prototype, a detached literal, …).
function _lumen_reflect_nid(self) {
    var n = (self === null || self === undefined) ? undefined : self.__nid__;
    return (n === null || n === undefined) ? -1 : n;
}

function _lumen_define_reflection(proto, entry) {
    var idl = entry[0], attr = entry[1], kind = entry[2], extra = entry[3];
    var get, set;
    if (kind === 'bool') {
        get = function() {
            var n = _lumen_reflect_nid(this);
            return n === -1 ? false : _lumen_has_attr(n, attr);
        };
        set = function(v) {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return;
            if (v) _lumen_set_attr(n, attr, ''); else _lumen_remove_attr(n, attr);
        };
    } else if (kind === 'long' || kind === 'ulong') {
        get = function() {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return extra;
            var p = _lumen_parse_integer(_lumen_u2n(_lumen_get_attr(n, attr)));
            if (p === null) return extra;
            var lo = (kind === 'ulong') ? 0 : _LUMEN_LONG_MIN;
            if (p < lo || p > _LUMEN_LONG_MAX) return extra;
            return p;
        };
        set = function(v) {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return;
            var p = Number(v);
            p = isFinite(p) ? Math.trunc(p) : 0;
            var lo = (kind === 'ulong') ? 0 : _LUMEN_LONG_MIN;
            if (p < lo || p > _LUMEN_LONG_MAX) p = extra;
            _lumen_set_attr(n, attr, String(p));
        };
    } else if (kind === 'url') {
        get = function() {
            var n = _lumen_reflect_nid(this);
            return n === -1 ? '' : _lumen_reflect_url(n, attr);
        };
        set = function(v) {
            var n = _lumen_reflect_nid(this);
            if (n !== -1) _lumen_set_attr(n, attr, String(v));
        };
    } else if (kind === 'enum') {
        get = function() {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return extra.def;
            var v = _lumen_u2n(_lumen_get_attr(n, attr));
            if (v === null) return extra.def;
            v = String(v).toLowerCase();
            for (var i = 0; i < extra.keys.length; i++) if (extra.keys[i] === v) return v;
            return extra.def;
        };
        set = function(v) {
            var n = _lumen_reflect_nid(this);
            if (n !== -1) _lumen_set_attr(n, attr, String(v));
        };
    } else {
        get = function() {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return '';
            var v = _lumen_u2n(_lumen_get_attr(n, attr));
            return v !== null ? String(v) : '';
        };
        set = function(v) {
            var n = _lumen_reflect_nid(this);
            if (n !== -1) _lumen_set_attr(n, attr, String(v));
        };
    }
    Object.defineProperty(proto, idl, { get: get, set: set, enumerable: true, configurable: true });
}

function _lumen_install_reflection(proto, entries) {
    if (!proto) return;
    for (var i = 0; i < entries.length; i++) _lumen_define_reflection(proto, entries[i]);
}

// Interfaces the tag→prototype table (BUG-322) did not carry yet. Without them
// `<area>`/`<optgroup>`/`<fieldset>`/… would fall back to `HTMLElement.prototype`
// and their reflected attributes would land on every element instead. Same shape
// as the generated block next to `HTMLElement` further up.
['HTMLAreaElement','HTMLOptGroupElement','HTMLFieldSetElement','HTMLLegendElement',
 'HTMLSourceElement','HTMLTrackElement','HTMLTimeElement','HTMLBaseElement',
 'HTMLOutputElement','HTMLDetailsElement','HTMLEmbedElement','HTMLObjectElement',
 'HTMLSlotElement','HTMLDataElement','HTMLQuoteElement','HTMLModElement',
 'HTMLProgressElement','HTMLMeterElement','HTMLMapElement','HTMLPictureElement',
 'HTMLTableColElement','HTMLTableCaptionElement','HTMLDListElement','HTMLMenuElement',
 // BUG-854: `<frame>` is obsolete but still parsed and still interface-bearing
 // (HTML LS §16.3.3), while `<frameset>` next door already had its interface.
 'HTMLFrameElement'
].forEach(function(_name) {
    if (_name in globalThis) return;
    var _ctor = function() { throw new TypeError('Illegal constructor'); };
    Object.defineProperty(_ctor, 'name', { value: _name, configurable: true });
    _ctor.prototype = Object.create(HTMLElement.prototype);
    _ctor.prototype.constructor = _ctor;
    globalThis[_name] = _ctor;
});
_lumen_html_tag_prototypes['AREA']      = HTMLAreaElement;
_lumen_html_tag_prototypes['OPTGROUP']  = HTMLOptGroupElement;
_lumen_html_tag_prototypes['FIELDSET']  = HTMLFieldSetElement;
_lumen_html_tag_prototypes['LEGEND']    = HTMLLegendElement;
_lumen_html_tag_prototypes['SOURCE']    = HTMLSourceElement;
_lumen_html_tag_prototypes['TRACK']     = HTMLTrackElement;
_lumen_html_tag_prototypes['TIME']      = HTMLTimeElement;
_lumen_html_tag_prototypes['BASE']      = HTMLBaseElement;
_lumen_html_tag_prototypes['OUTPUT']    = HTMLOutputElement;
_lumen_html_tag_prototypes['DETAILS']   = HTMLDetailsElement;
_lumen_html_tag_prototypes['EMBED']     = HTMLEmbedElement;
_lumen_html_tag_prototypes['OBJECT']    = HTMLObjectElement;
_lumen_html_tag_prototypes['SLOT']      = HTMLSlotElement;
_lumen_html_tag_prototypes['DATA']      = HTMLDataElement;
_lumen_html_tag_prototypes['Q']          = HTMLQuoteElement;
_lumen_html_tag_prototypes['BLOCKQUOTE'] = HTMLQuoteElement;
_lumen_html_tag_prototypes['INS']       = HTMLModElement;
_lumen_html_tag_prototypes['DEL']       = HTMLModElement;
_lumen_html_tag_prototypes['PROGRESS']  = HTMLProgressElement;
_lumen_html_tag_prototypes['METER']     = HTMLMeterElement;
_lumen_html_tag_prototypes['MAP']       = HTMLMapElement;
_lumen_html_tag_prototypes['PICTURE']   = HTMLPictureElement;
_lumen_html_tag_prototypes['COL']       = HTMLTableColElement;
_lumen_html_tag_prototypes['COLGROUP']  = HTMLTableColElement;
_lumen_html_tag_prototypes['CAPTION']   = HTMLTableCaptionElement;
_lumen_html_tag_prototypes['DL']        = HTMLDListElement;
_lumen_html_tag_prototypes['MENU']      = HTMLMenuElement;
_lumen_html_tag_prototypes['FRAME']     = HTMLFrameElement;

// `referrerpolicy` shares one keyword set across <a>/<area>/<img>/<iframe>/…
var _LUMEN_REFERRER_POLICY = { def: '', keys: [
    'no-referrer', 'no-referrer-when-downgrade', 'same-origin', 'origin',
    'strict-origin', 'origin-when-cross-origin', 'strict-origin-when-cross-origin',
    'unsafe-url'] };

// Global attributes (HTML LS §3.2.6) — every HTML element has them.
// `id`, `className`, `slot` and `draggable` stay own properties on the wrapper
// (they predate this table and carry extra behaviour), so they are not repeated.
_lumen_install_reflection(HTMLElement.prototype, [
    ['title',          'title',          'string'],
    ['lang',           'lang',           'string'],
    ['dir',            'dir',            'enum',   { def: '', keys: ['ltr', 'rtl', 'auto'] }],
    ['hidden',         'hidden',         'bool'],
    ['inert',          'inert',          'bool'],
    ['accessKey',      'accesskey',      'string'],
    ['autocapitalize', 'autocapitalize', 'string'],
    ['enterKeyHint',   'enterkeyhint',   'string'],
    ['inputMode',      'inputmode',      'string'],
    ['nonce',          'nonce',          'string'],
]);

_lumen_install_reflection(HTMLAnchorElement.prototype, [
    ['href',           'href',           'url'],
    ['target',         'target',         'string'],
    ['download',       'download',       'string'],
    ['rel',            'rel',            'string'],
    ['hreflang',       'hreflang',       'string'],
    ['type',           'type',           'string'],
    ['ping',           'ping',           'string'],
    ['referrerPolicy', 'referrerpolicy', 'enum',   _LUMEN_REFERRER_POLICY],
]);

_lumen_install_reflection(HTMLAreaElement.prototype, [
    ['href',           'href',           'url'],
    ['alt',            'alt',            'string'],
    ['coords',         'coords',         'string'],
    ['shape',          'shape',          'string'],
    ['target',         'target',         'string'],
    ['download',       'download',       'string'],
    ['rel',            'rel',            'string'],
    ['ping',           'ping',           'string'],
    ['referrerPolicy', 'referrerpolicy', 'enum',   _LUMEN_REFERRER_POLICY],
]);

// HTML LS §4.6.3 `HTMLHyperlinkElementUtils` mixin (BUG-356 part 2 — part 1,
// `href` reflection above, was already wired). Every accessor reads through to
// the live `href` content attribute (resolved against the document base URL)
// on each call rather than caching, since element wrappers are interned per
// nid (`_lumen_element_wrappers`) and outlive attribute mutations.
function _lumen_hyperlink_url_get(self) {
    var n = _lumen_reflect_nid(self);
    if (n === -1) return null;
    var abs = _lumen_reflect_url(n, 'href');
    if (abs === '') return null;
    return _lumen_parse_url(abs);
}
// Decompose the current href, let `mutate` edit the parts, re-serialize and
// write the result back to the `href` attribute. Per spec, setting a part
// when the element has no href (or isn't live) is a no-op.
function _lumen_hyperlink_url_set(self, mutate) {
    var n = _lumen_reflect_nid(self);
    if (n === -1) return;
    var abs = _lumen_reflect_url(n, 'href');
    if (abs === '') return;
    var p = _lumen_parse_url(abs);
    mutate(p);
    var creds = '';
    if (p.username || p.password) creds = p.username + (p.password ? ':' + p.password : '') + '@';
    var authority = creds + p.hostname + (p.port ? ':' + p.port : '');
    _lumen_set_attr(n, 'href', p.protocol + '//' + authority + (p.pathname || '/') + (p.search || '') + (p.hash || ''));
}
function _lumen_install_hyperlink_utils(proto) {
    function part(idl, mutate) {
        Object.defineProperty(proto, idl, {
            get: function() { var p = _lumen_hyperlink_url_get(this); return p ? p[idl] : ''; },
            set: function(v) { _lumen_hyperlink_url_set(this, function(p) { mutate(p, String(v)); }); },
            enumerable: true, configurable: true
        });
    }
    part('protocol', function(p, v) { p.protocol = v.replace(/:.*$/, '') + ':'; });
    part('hostname', function(p, v) { p.hostname = v.split(':')[0].split('/')[0]; });
    part('host',     function(p, v) {
        var idx = v.indexOf(':');
        p.hostname = idx >= 0 ? v.slice(0, idx) : v;
        p.port     = idx >= 0 ? v.slice(idx + 1) : '';
    });
    part('port',     function(p, v) { p.port = v.replace(/\D/g, ''); });
    part('pathname', function(p, v) { p.pathname = v.charAt(0) === '/' ? v : '/' + v; });
    part('search',   function(p, v) { p.search = v && v.charAt(0) !== '?' ? '?' + v : v; });
    part('hash',     function(p, v) { p.hash = v && v.charAt(0) !== '#' ? '#' + v : v; });
    Object.defineProperty(proto, 'origin', {
        get: function() { var p = _lumen_hyperlink_url_get(this); return p ? p.origin : ''; },
        enumerable: true, configurable: true
    });
    // Credentials come out of the same parse as the other components (BUG-375
    // taught `_lumen_parse_url` about userinfo), so these are real accessors
    // rather than the inert stubs they used to be.
    part('username', function(p, v) { p.username = _lumen_url_userinfo_encode(v); });
    part('password', function(p, v) { p.password = _lumen_url_userinfo_encode(v); });
}
_lumen_install_hyperlink_utils(HTMLAnchorElement.prototype);
_lumen_install_hyperlink_utils(HTMLAreaElement.prototype);

_lumen_install_reflection(HTMLInputElement.prototype, [
    ['type',           'type',           'enum',   { def: 'text', keys: [
        'button', 'checkbox', 'color', 'date', 'datetime-local', 'email', 'file',
        'hidden', 'image', 'month', 'number', 'password', 'radio', 'range',
        'reset', 'search', 'submit', 'tel', 'text', 'time', 'url', 'week'] }],
    ['name',           'name',           'string'],
    ['disabled',       'disabled',       'bool'],
    ['readOnly',       'readonly',       'bool'],
    ['required',       'required',       'bool'],
    ['multiple',       'multiple',       'bool'],
    ['placeholder',    'placeholder',    'string'],
    ['pattern',        'pattern',        'string'],
    ['accept',         'accept',         'string'],
    ['alt',            'alt',            'string'],
    ['autocomplete',   'autocomplete',   'string'],
    ['capture',        'capture',        'string'],
    ['min',            'min',            'string'],
    ['max',            'max',            'string'],
    ['step',           'step',           'string'],
    ['src',            'src',            'url'],
    ['dirName',        'dirname',        'string'],
    ['maxLength',      'maxlength',      'long',   -1],
    ['minLength',      'minlength',      'long',   -1],
    ['size',           'size',           'ulong',  20],
    ['defaultValue',   'value',          'string'],
    // BUG-444: `checked` itself is not plain reflection — it is the current
    // checkedness (`Document::dirty_checkedness`, custom accessor elsewhere)
    // — but `defaultChecked` genuinely is: the `checked` content attribute
    // IS the default, same shape as `defaultSelected` below.
    ['defaultChecked', 'checked',        'bool'],
    ['formAction',     'formaction',     'url'],
    ['formEnctype',    'formenctype',    'string'],
    ['formMethod',     'formmethod',     'enum',   { def: '', keys: ['get', 'post', 'dialog'] }],
    ['formTarget',     'formtarget',     'string'],
    ['formNoValidate', 'formnovalidate', 'bool'],
]);

_lumen_install_reflection(HTMLTextAreaElement.prototype, [
    ['name',           'name',           'string'],
    ['disabled',       'disabled',       'bool'],
    ['readOnly',       'readonly',       'bool'],
    ['required',       'required',       'bool'],
    ['placeholder',    'placeholder',    'string'],
    ['autocomplete',   'autocomplete',   'string'],
    ['dirName',        'dirname',        'string'],
    ['wrap',           'wrap',           'string'],
    ['rows',           'rows',           'ulong',  2],
    ['cols',           'cols',           'ulong',  20],
    ['maxLength',      'maxlength',      'long',   -1],
    ['minLength',      'minlength',      'long',   -1],
]);

_lumen_install_reflection(HTMLSelectElement.prototype, [
    ['name',           'name',           'string'],
    ['disabled',       'disabled',       'bool'],
    ['required',       'required',       'bool'],
    ['multiple',       'multiple',       'bool'],
    ['autocomplete',   'autocomplete',   'string'],
    ['size',           'size',           'ulong',  0],
]);

_lumen_install_reflection(HTMLOptionElement.prototype, [
    ['disabled',        'disabled',      'bool'],
    ['defaultSelected', 'selected',      'bool'],
]);

_lumen_install_reflection(HTMLOptGroupElement.prototype, [
    ['disabled',       'disabled',       'bool'],
    ['label',          'label',          'string'],
]);

_lumen_install_reflection(HTMLButtonElement.prototype, [
    ['name',           'name',           'string'],
    ['disabled',       'disabled',       'bool'],
    ['type',           'type',           'enum',   { def: 'submit', keys: ['submit', 'reset', 'button'] }],
    ['formAction',     'formaction',     'url'],
    ['formEnctype',    'formenctype',    'string'],
    ['formMethod',     'formmethod',     'enum',   { def: '', keys: ['get', 'post', 'dialog'] }],
    ['formTarget',     'formtarget',     'string'],
    ['formNoValidate', 'formnovalidate', 'bool'],
]);

_lumen_install_reflection(HTMLFormElement.prototype, [
    ['name',           'name',           'string'],
    ['action',         'action',         'url'],
    ['method',         'method',         'enum',   { def: 'get', keys: ['get', 'post', 'dialog'] }],
    ['enctype',        'enctype',        'string'],
    ['encoding',       'enctype',        'string'],
    ['target',         'target',         'string'],
    ['acceptCharset',  'accept-charset', 'string'],
    ['autocomplete',   'autocomplete',   'string'],
    ['rel',            'rel',            'string'],
    ['noValidate',     'novalidate',     'bool'],
]);

_lumen_install_reflection(HTMLFieldSetElement.prototype, [
    ['name',           'name',           'string'],
    ['disabled',       'disabled',       'bool'],
]);

_lumen_install_reflection(HTMLLabelElement.prototype, [
    ['htmlFor',        'for',            'string'],
]);

_lumen_install_reflection(HTMLOutputElement.prototype, [
    ['name',           'name',           'string'],
    ['htmlFor',        'for',            'string'],
]);

_lumen_install_reflection(HTMLImageElement.prototype, [
    ['src',            'src',            'url'],
    ['alt',            'alt',            'string'],
    ['srcset',         'srcset',         'string'],
    ['sizes',          'sizes',          'string'],
    ['useMap',         'usemap',         'string'],
    ['isMap',          'ismap',          'bool'],
    ['crossOrigin',    'crossorigin',    'string'],
    ['decoding',       'decoding',       'string'],
    ['loading',        'loading',        'string'],
    ['referrerPolicy', 'referrerpolicy', 'enum',   _LUMEN_REFERRER_POLICY],
]);

// BUG-450: `width`/`height` are not a global attribute pair — until the canvas
// members moved onto `HTMLCanvasElement.prototype` they were served to EVERY
// element by the shared wrapper table, which is why `document.createElement('div')
// .width = 42` wrote a `width` attribute. The interfaces below are the complete
// set HTML LS gives the pair to (checked against `tests/wpt/interfaces/html.idl`),
// and the type differs per interface: `unsigned long` on the four modern ones,
// `DOMString` on the obsolete-but-parsed ones, where `<td width="5">` must read
// back as the string `'5'` and not the number 5. `<canvas>` is deliberately absent
// — its pair resizes a bitmap and is defined next to `getContext`.
[HTMLImageElement.prototype, HTMLVideoElement.prototype,
 HTMLInputElement.prototype, HTMLSourceElement.prototype].forEach(function(_p) {
    _lumen_install_reflection(_p, [
        ['width',      'width',      'ulong', 0],
        ['height',     'height',     'ulong', 0],
    ]);
});
// `<iframe>` is patched per element by `iframe_element.rs` with the same string
// semantics; that own property keeps shadowing this row, which exists for the
// iframes that patch never reaches (`innerHTML`, `createElementNS`).
// (`HTMLMarqueeElement` owns the pair too but has no interface in this engine,
// so `<marquee>` is left without it rather than growing an interface here.)
[HTMLIFrameElement.prototype, HTMLEmbedElement.prototype,
 HTMLObjectElement.prototype].forEach(function(_p) {
    _lumen_install_reflection(_p, [
        ['width',      'width',      'string'],
        ['height',     'height',     'string'],
    ]);
});
_lumen_install_reflection(HTMLTableCellElement.prototype, [
    ['width',          'width',          'string'],
    ['height',         'height',         'string'],
]);
// `width` alone — these three carry no obsolete `height` IDL attribute.
[HTMLTableColElement.prototype, HTMLTableElement.prototype,
 HTMLHRElement.prototype].forEach(function(_p) {
    _lumen_install_reflection(_p, [['width', 'width', 'string']]);
});
_lumen_install_reflection(HTMLPreElement.prototype, [['width', 'width', 'long', 0]]);

_lumen_install_reflection(HTMLScriptElement.prototype, [
    ['src',            'src',            'url'],
    ['type',           'type',           'string'],
    ['async',          'async',          'bool'],
    ['defer',          'defer',          'bool'],
    ['noModule',       'nomodule',       'bool'],
    ['crossOrigin',    'crossorigin',    'string'],
    ['integrity',      'integrity',      'string'],
    ['referrerPolicy', 'referrerpolicy', 'enum',   _LUMEN_REFERRER_POLICY],
]);

_lumen_install_reflection(HTMLLinkElement.prototype, [
    ['href',           'href',           'url'],
    ['rel',            'rel',            'string'],
    ['media',          'media',          'string'],
    ['hreflang',       'hreflang',       'string'],
    ['type',           'type',           'string'],
    ['as',             'as',             'string'],
    ['sizes',          'sizes',          'string'],
    ['imageSrcset',    'imagesrcset',    'string'],
    ['imageSizes',     'imagesizes',     'string'],
    ['crossOrigin',    'crossorigin',    'string'],
    ['integrity',      'integrity',      'string'],
    ['referrerPolicy', 'referrerpolicy', 'enum',   _LUMEN_REFERRER_POLICY],
]);

// BUG-826: `relList` is the DOMTokenList over the `rel` attribute, and its
// `supports()` is how a page feature-detects a link type — WPT's own
// `preload_helper.js` opens with `link.relList.supports('preload')` and throws
// out of every preload test when the member is missing. Declared per interface
// (not in the shared wrapper table), because the supported-token set differs
// between `<link>` and `<a>` and a shared entry would outrank both (BUG-796).
var _LUMEN_LINK_REL_TOKENS = [
    'alternate', 'canonical', 'author', 'dns-prefetch', 'expect', 'help',
    'icon', 'license', 'manifest', 'modulepreload', 'next', 'pingback',
    'preconnect', 'prefetch', 'preload', 'prev', 'privacy-policy', 'search',
    'stylesheet', 'terms-of-service'
];
var _LUMEN_ANCHOR_REL_TOKENS = [
    'alternate', 'author', 'bookmark', 'external', 'help', 'license', 'next',
    'nofollow', 'noopener', 'noreferrer', 'opener', 'prev', 'privacy-policy',
    'search', 'tag', 'terms-of-service'
];

function _lumen_make_rel_list(nid, supported) {
    var rl = _lumen_make_attr_token_list(nid, 'rel');
    rl.supports = function(token) {
        return supported.indexOf(String(token).toLowerCase()) >= 0;
    };
    return rl;
}
function _lumen_make_link_rel_list(nid) {
    return _lumen_make_rel_list(nid, _LUMEN_LINK_REL_TOKENS);
}
function _lumen_make_anchor_rel_list(nid) {
    return _lumen_make_rel_list(nid, _LUMEN_ANCHOR_REL_TOKENS);
}

// Cached in a wrapper slot exactly like `classList`, so `el.relList === el.relList`
// holds and a token added through one read is visible through the next.
Object.defineProperty(HTMLLinkElement.prototype, 'relList', {
    get: function() {
        if (_lumen_reflect_nid(this) === -1) return null;
        return _lumen_wrapper_slot(this, '__relList__', _lumen_make_link_rel_list);
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLAnchorElement.prototype, 'relList', {
    get: function() {
        if (_lumen_reflect_nid(this) === -1) return null;
        return _lumen_wrapper_slot(this, '__relList__', _lumen_make_anchor_rel_list);
    },
    enumerable: true, configurable: true,
});

_lumen_install_reflection(HTMLStyleElement.prototype, [
    ['media',          'media',          'string'],
    ['type',           'type',           'string'],
]);

_lumen_install_reflection(HTMLIFrameElement.prototype, [
    ['src',            'src',            'url'],
    ['srcdoc',         'srcdoc',         'string'],
    ['name',           'name',           'string'],
    ['allow',          'allow',          'string'],
    ['allowFullscreen','allowfullscreen','bool'],
    ['loading',        'loading',        'string'],
    ['referrerPolicy', 'referrerpolicy', 'enum',   _LUMEN_REFERRER_POLICY],
]);

// BUG-854 — HTML LS §16.3.3 `HTMLFrameElement`. Obsolete, still parsed, and a
// real nested browsing context host: WPT's own `name-attribute.window.js` opens
// with «frame — this works without <frameset>, so great», i.e. the element is
// loaded wherever it stands, not only inside a frameset.
_lumen_install_reflection(HTMLFrameElement.prototype, [
    ['src',          'src',          'url'],
    ['name',         'name',         'string'],
    ['scrolling',    'scrolling',    'string'],
    ['frameBorder',  'frameborder',  'string'],
    ['longDesc',     'longdesc',     'url'],
    ['noResize',     'noresize',     'bool'],
    ['marginHeight', 'marginheight', 'string'],
    ['marginWidth',  'marginwidth',  'string'],
]);
// `contentDocument`/`contentWindow` read the same sub-document registry as
// `<iframe>` (BUG-480 срез 2, `frame_bridge.rs`) — the shell registers a
// binding per host node id regardless of the host's tag. Installed on the
// **prototype**, unlike `iframe_element.rs`'s per-element patch: a `<frame>`
// written by the parser never passes through a `createElement` hook, and the
// prototype covers both origins with one definition. `typeof`-guard keeps the
// shim working in runtimes installed without the bridge (`--dump-*`, SVG
// rasterization, unit tests).
_lumen_frame_define_content_accessors(HTMLFrameElement.prototype);
function _lumen_frame_define_content_accessors(proto) {
    Object.defineProperty(proto, 'contentDocument', {
        get: function() {
            var n = _lumen_reflect_nid(this);
            return (typeof _lumen_frame_content_document === 'function' && n !== -1)
                ? _lumen_frame_content_document(n)
                : null;
        },
        enumerable: true,
        configurable: true,
    });
    Object.defineProperty(proto, 'contentWindow', {
        get: function() {
            var n = _lumen_reflect_nid(this);
            return (typeof _lumen_frame_content_window === 'function' && n !== -1)
                ? _lumen_frame_content_window(n)
                : null;
        },
        enumerable: true,
        configurable: true,
    });
}

// `scheme` is the obsolete-but-conforming member (HTML LS §16.3); `charset` is a
// content attribute with NO IDL counterpart on this interface — checked against
// `tests/wpt/interfaces/html.idl`, which BUG-796 got backwards.
_lumen_install_reflection(HTMLMetaElement.prototype, [
    ['name',           'name',           'string'],
    ['content',        'content',        'string'],
    ['httpEquiv',      'http-equiv',     'string'],
    ['media',          'media',          'string'],
    ['scheme',         'scheme',         'string'],
]);

_lumen_install_reflection(HTMLTableCellElement.prototype, [
    ['colSpan',        'colspan',        'ulong',  1],
    ['rowSpan',        'rowspan',        'ulong',  1],
    ['headers',        'headers',        'string'],
    ['abbr',           'abbr',           'string'],
    ['scope',          'scope',          'string'],
]);

_lumen_install_reflection(HTMLTableColElement.prototype, [
    ['span',           'span',           'ulong',  1],
]);

_lumen_install_reflection(HTMLOListElement.prototype, [
    ['reversed',       'reversed',       'bool'],
    ['start',          'start',          'long',   1],
    ['type',           'type',           'string'],
]);

_lumen_install_reflection(HTMLTimeElement.prototype, [
    ['dateTime',       'datetime',       'string'],
]);

_lumen_install_reflection(HTMLQuoteElement.prototype, [
    ['cite',           'cite',           'url'],
]);

_lumen_install_reflection(HTMLModElement.prototype, [
    ['cite',           'cite',           'url'],
    ['dateTime',       'datetime',       'string'],
]);

_lumen_install_reflection(HTMLTrackElement.prototype, [
    ['kind',           'kind',           'enum',   { def: 'subtitles', keys: [
        'subtitles', 'captions', 'descriptions', 'chapters', 'metadata'] }],
    ['src',            'src',            'url'],
    ['srclang',        'srclang',        'string'],
    ['label',          'label',          'string'],
    ['default',        'default',        'bool'],
]);

_lumen_install_reflection(HTMLSourceElement.prototype, [
    ['src',            'src',            'url'],
    ['type',           'type',           'string'],
    ['srcset',         'srcset',         'string'],
    ['sizes',          'sizes',          'string'],
    ['media',          'media',          'string'],
]);

// <video>/<audio> share HTMLMediaElement's attributes; Lumen has no
// HTMLMediaElement interface yet, so the rows are installed on both. <audio>
// additionally carries its own `src` accessor (`audio_element.rs`) that drives
// the media loader — an own property, so it keeps shadowing the row below.
[HTMLVideoElement.prototype, HTMLAudioElement.prototype].forEach(function(_p) {
    _lumen_install_reflection(_p, [
        ['src',          'src',          'url'],
        ['preload',      'preload',      'string'],
        ['crossOrigin',  'crossorigin',  'string'],
        ['autoplay',     'autoplay',     'bool'],
        ['loop',         'loop',         'bool'],
        ['controls',     'controls',     'bool'],
        ['defaultMuted', 'muted',        'bool'],
        ['playsInline',  'playsinline',  'bool'],
    ]);
});
_lumen_install_reflection(HTMLVideoElement.prototype, [
    ['poster',         'poster',         'url'],
]);

_lumen_install_reflection(HTMLObjectElement.prototype, [
    ['data',           'data',           'url'],
    ['type',           'type',           'string'],
    ['name',           'name',           'string'],
    ['useMap',         'usemap',         'string'],
]);

_lumen_install_reflection(HTMLEmbedElement.prototype, [
    ['src',            'src',            'url'],
    ['type',           'type',           'string'],
]);

_lumen_install_reflection(HTMLMapElement.prototype,     [['name', 'name', 'string']]);
_lumen_install_reflection(HTMLSlotElement.prototype,    [['name', 'name', 'string']]);
_lumen_install_reflection(HTMLDetailsElement.prototype, [['name', 'name', 'string']]);

// HTML LS §4.2.3: `base.href` resolves against the *document* URL, never against
// the base URL it is itself defining — so it cannot be an ordinary `url` row.
Object.defineProperty(HTMLBaseElement.prototype, 'href', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        var docUrl = '';
        try {
            if (typeof location !== 'undefined' && location.href) docUrl = String(location.href);
        } catch (e) {}
        if (n === -1) return docUrl;
        var v = _lumen_u2n(_lumen_get_attr(n, 'href'));
        if (v === null || String(v) === '') return docUrl;
        return _url_resolve(String(v), docUrl);
    },
    set: function(v) {
        var n = _lumen_reflect_nid(this);
        if (n !== -1) _lumen_set_attr(n, 'href', String(v));
    },
    enumerable: true, configurable: true,
});
_lumen_install_reflection(HTMLBaseElement.prototype, [['target', 'target', 'string']]);

// `text` is a child-text alias rather than an attribute reflection, so it is
// defined by hand: HTML LS §4.5.1 (`a.text`), §4.12.1 (`script.text`).
[HTMLAnchorElement.prototype, HTMLScriptElement.prototype].forEach(function(_p) {
    Object.defineProperty(_p, 'text', {
        get: function() {
            var n = _lumen_reflect_nid(this);
            return n === -1 ? '' : _lumen_get_text_content(n);
        },
        set: function(v) {
            var n = _lumen_reflect_nid(this);
            if (n !== -1) _lumen_set_text_content(n, String(v));
        },
        enumerable: true, configurable: true,
    });
});

// `type` is fixed for these two interfaces (HTML LS §4.10.7/§4.10.11) — they
// have no `type` content attribute at all, so it is a read-only getter.
Object.defineProperty(HTMLTextAreaElement.prototype, 'type', {
    get: function() { return 'textarea'; }, enumerable: true, configurable: true,
});
Object.defineProperty(HTMLSelectElement.prototype, 'type', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        return (n !== -1 && _lumen_has_attr(n, 'multiple')) ? 'select-multiple' : 'select-one';
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLFieldSetElement.prototype, 'type', {
    get: function() { return 'fieldset'; }, enumerable: true, configurable: true,
});

// HTML LS §4.12.3 — `readonly attribute DocumentFragment content`, an IDL
// attribute of HTMLTemplateElement and of nothing else. It used to be declared in
// `_LUMEN_WRAPPER_MEMBERS`, i.e. on the shared wrapper prototype every element
// gets, where its «not a template → undefined» branch shadowed the reflected
// `HTMLMetaElement.content` (and would shadow any future `content` on another
// interface) — BUG-796. `testharness.js` reads `meta.content` to pick its own
// timeout, so that shadow cost every `<meta name=timeout content=long>` test the
// long ceiling. Placed here, only a `<template>` wrapper can reach it, because
// `_lumen_element_prototype_for` hands out this prototype for the TEMPLATE tag
// alone.
Object.defineProperty(HTMLTemplateElement.prototype, 'content', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return null;
        // `[SameObject]`: the fragment NODE has been stable since BUG-368 (the
        // native side creates it once and remembers it, `createElement`-built
        // templates included), but `_lumen_make_document_fragment` mints a fresh
        // literal per call, so `t.content !== t.content` held and an expando
        // written on the fragment was lost. The wrapper is cached on the element
        // wrapper, which is itself interned per nid (BUG-291).
        var cached = this.__templateContent__;
        if (cached !== undefined) return cached;
        var frag_nid = _lumen_u2n(_lumen_get_template_content(n));
        if (frag_nid === null) return null;
        var frag = _lumen_make_document_fragment(frag_nid);
        _lumen_wrapper_set_slot(this, '__templateContent__', frag);
        return frag;
    },
    enumerable: true, configurable: true,
});

// ── Form-control associations, collections and activation (BUG-383) ──────────
// Everything below is the non-reflection half of the same gap: `form.elements`
// was a plain `Array` (so `namedItem`/named access silently did nothing),
// `select.options`/`selectedIndex` did not exist, and no element had `click()`,
// `select()` or `setSelectionRange()` — the standard ways to drive a form from
// script and the ones every polyfill and e2e helper reaches for first.

// Tags that are «listed elements» for the purposes of `form.elements`
// (HTML LS §4.10.3). `<option>` is deliberately absent: it belongs to
// `select.options`, not to the form's control list.
var _LUMEN_LISTED_TAGS = {
    INPUT: 1, SELECT: 1, TEXTAREA: 1, BUTTON: 1, FIELDSET: 1, OBJECT: 1, OUTPUT: 1,
};
// Controls a <label> can label (HTML LS §4.10.4 «labelable elements»).
var _LUMEN_LABELABLE_TAGS = {
    INPUT: 1, SELECT: 1, TEXTAREA: 1, BUTTON: 1, METER: 1, OUTPUT: 1, PROGRESS: 1,
};
// `<input>` types with a text-entry cursor, i.e. the ones whose selection API
// is defined (HTML LS §4.10.5.4). Everything else answers `null`.
var _LUMEN_SELECTABLE_INPUT_TYPES = {
    text: 1, search: 1, url: 1, tel: 1, password: 1,
};

// nid → true/false override of an <option>'s selectedness, set by
// `option.selected = …` / `select.selectedIndex = …`. Absent means «follow the
// `selected` content attribute», which is the markup default.
var _lumen_option_selected = {};
// nid → { start, end, direction } for text-entry controls.
var _lumen_selection_state = {};
// nid → indeterminate flag for checkboxes (never reflected as an attribute).
var _lumen_indeterminate = {};
// nid → true while an activation behaviour is running on that node, so a
// `click()` inside a click handler for the same element cannot recurse forever
// (label → control → label is the usual cycle).
var _lumen_click_in_progress = {};

// Depth-first element descendants of `nid`, in tree order.
function _lumen_descendant_elements(nid, out) {
    var kids = _lumen_get_children(nid);
    for (var i = 0; i < kids.length; i++) {
        if (_lumen_is_text_node(kids[i]) || _lumen_is_comment_node(kids[i])) continue;
        out.push(kids[i]);
        _lumen_descendant_elements(kids[i], out);
    }
    return out;
}

// HTML LS §4.10.18.6 «form owner»: the form named by the control's `form`
// content attribute, else the nearest ancestor <form>.
function _lumen_form_owner(nid) {
    var formId = _lumen_u2n(_lumen_get_attr(nid, 'form'));
    if (formId !== null && String(formId) !== '') {
        var byId = _lumen_u2n(_lumen_get_element_by_id(String(formId)));
        if (byId !== null && (_lumen_get_tag_name(byId) || '').toUpperCase() === 'FORM') return byId;
        return -1;
    }
    var cur = _lumen_u2n(_lumen_get_parent(nid));
    for (var guard = 0; guard < 512 && cur !== null; guard++) {
        if ((_lumen_get_tag_name(cur) || '').toUpperCase() === 'FORM') return cur;
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    return -1;
}

// The controls a <form> or <fieldset> owns, in tree order.
function _lumen_listed_controls(owner_nid) {
    var all = _lumen_descendant_elements(owner_nid, []);
    var out = [];
    var ownerIsForm = (_lumen_get_tag_name(owner_nid) || '').toUpperCase() === 'FORM';
    for (var i = 0; i < all.length; i++) {
        var tag = (_lumen_get_tag_name(all[i]) || '').toUpperCase();
        if (_LUMEN_LISTED_TAGS[tag] !== 1) continue;
        // A control inside the subtree but re-parented by a `form` attribute
        // belongs to that other form, not to this one.
        if (ownerIsForm && _lumen_form_owner(all[i]) !== owner_nid) continue;
        out.push(all[i]);
    }
    return out;
}

// HTML LS §4.10.3 — `HTMLFormControlsCollection`, the interface `form.elements`
// is supposed to return. It was a plain `Array` before, which silently lacked
// `namedItem()` and named access while `Array` methods made it look complete.
function HTMLFormControlsCollection() { throw new TypeError('Illegal constructor'); }
HTMLFormControlsCollection.prototype = Object.create(HTMLCollection.prototype);
HTMLFormControlsCollection.prototype.constructor = HTMLFormControlsCollection;
window.HTMLFormControlsCollection = HTMLFormControlsCollection;

// HTML LS §4.10.7 — `HTMLOptionsCollection`, returned by `select.options`.
function HTMLOptionsCollection() { throw new TypeError('Illegal constructor'); }
HTMLOptionsCollection.prototype = Object.create(HTMLCollection.prototype);
HTMLOptionsCollection.prototype.constructor = HTMLOptionsCollection;
window.HTMLOptionsCollection = HTMLOptionsCollection;

// `for (var c of form.elements)` / `[...select.options]` — the indexed getter
// alone does not make a legacy platform object iterable. `Symbol.toStringTag`
// goes on at the same time so `Object.prototype.toString.call(...)` names the
// interface instead of answering the `[object Object]` that made the old
// `Array` masquerade so hard to spot.
[['HTMLCollection', HTMLCollection.prototype],
 // BUG-412: `getElementsByName` hands out a NodeList, so `[...list]` and
 // `Object.prototype.toString.call(list)` must work on it too.
 ['NodeList', NodeList.prototype],
 ['HTMLFormControlsCollection', HTMLFormControlsCollection.prototype],
 ['HTMLOptionsCollection', HTMLOptionsCollection.prototype],
 // NamedNodeMap (BUG-732) is indexed the same way, so `[...el.attributes]`
 // and `for (var a of el.attributes)` come from the same iterator.
 ['NamedNodeMap', NamedNodeMap.prototype]].forEach(function(_e) {
    if (typeof Symbol === 'undefined' || !Symbol.iterator) return;
    var _p = _e[1];
    if (!_p[Symbol.iterator]) {
        Object.defineProperty(_p, Symbol.iterator, {
            value: function() {
                var self = this, i = 0;
                return { next: function() {
                    return i < self.length ? { value: self[i++], done: false }
                                           : { value: undefined, done: true };
                } };
            },
            writable: true, configurable: true, enumerable: false,
        });
    }
    if (Symbol.toStringTag) {
        Object.defineProperty(_p, Symbol.toStringTag, {
            value: _e[0], writable: false, configurable: true, enumerable: false,
        });
    }
});

[HTMLFormElement.prototype, HTMLFieldSetElement.prototype].forEach(function(_p) {
    Object.defineProperty(_p, 'elements', {
        get: function() {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return _lumen_make_nid_collection(function() { return []; },
                                                            HTMLFormControlsCollection.prototype);
            return _lumen_make_nid_collection(function() { return _lumen_listed_controls(n); },
                                              HTMLFormControlsCollection.prototype);
        },
        enumerable: true, configurable: true,
    });
});
Object.defineProperty(HTMLFormElement.prototype, 'length', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        return n === -1 ? 0 : _lumen_listed_controls(n).length;
    },
    enumerable: true, configurable: true,
});

// Form-owner back-references on the controls themselves (HTML LS §4.10.18.6).
['HTMLInputElement', 'HTMLSelectElement', 'HTMLTextAreaElement', 'HTMLButtonElement',
 'HTMLFieldSetElement', 'HTMLObjectElement', 'HTMLOutputElement', 'HTMLLabelElement',
 'HTMLLegendElement'].forEach(function(_iface) {
    Object.defineProperty(globalThis[_iface].prototype, 'form', {
        get: function() {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return null;
            var f = _lumen_form_owner(n);
            return f === -1 ? null : _lumen_make_element(f);
        },
        enumerable: true, configurable: true,
    });
});
// `<option>.form` is the form of its owning <select>, not its own ancestor form.
Object.defineProperty(HTMLOptionElement.prototype, 'form', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return null;
        var sel = _lumen_option_owner_select(n);
        if (sel === -1) return null;
        var f = _lumen_form_owner(sel);
        return f === -1 ? null : _lumen_make_element(f);
    },
    enumerable: true, configurable: true,
});

// HTML LS §4.10.4 — the <label>s associated with a control: every ancestor
// <label>, plus every `<label for=id>` pointing at it.
function _lumen_labels_for(nid) {
    var out = [];
    var id = _lumen_u2n(_lumen_get_attr(nid, 'id'));
    var cur = _lumen_u2n(_lumen_get_parent(nid));
    for (var guard = 0; guard < 512 && cur !== null; guard++) {
        if ((_lumen_get_tag_name(cur) || '').toUpperCase() === 'LABEL') out.push(cur);
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    if (id !== null && String(id) !== '') {
        var all = _lumen_descendant_elements(_lumen_root_nid, []);
        for (var i = 0; i < all.length; i++) {
            if ((_lumen_get_tag_name(all[i]) || '').toUpperCase() !== 'LABEL') continue;
            if (_lumen_u2n(_lumen_get_attr(all[i], 'for')) === String(id)) {
                if (out.indexOf(all[i]) === -1) out.push(all[i]);
            }
        }
    }
    return out;
}
['HTMLInputElement', 'HTMLSelectElement', 'HTMLTextAreaElement', 'HTMLButtonElement',
 'HTMLOutputElement', 'HTMLProgressElement', 'HTMLMeterElement'].forEach(function(_iface) {
    Object.defineProperty(globalThis[_iface].prototype, 'labels', {
        get: function() {
            var n = _lumen_reflect_nid(this);
            if (n === -1) return null;
            return _lumen_make_nid_collection(function() { return _lumen_labels_for(n); },
                                              HTMLCollection.prototype);
        },
        enumerable: true, configurable: true,
    });
});

// HTML LS §4.10.4 — `label.control`: the element named by `for`, else the first
// labelable descendant.
Object.defineProperty(HTMLLabelElement.prototype, 'control', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return null;
        var target = _lumen_u2n(_lumen_get_attr(n, 'for'));
        if (target !== null && String(target) !== '') {
            var byId = _lumen_u2n(_lumen_get_element_by_id(String(target)));
            if (byId === null) return null;
            var t = (_lumen_get_tag_name(byId) || '').toUpperCase();
            return _LUMEN_LABELABLE_TAGS[t] === 1 ? _lumen_make_element(byId) : null;
        }
        var desc = _lumen_descendant_elements(n, []);
        for (var i = 0; i < desc.length; i++) {
            if (_LUMEN_LABELABLE_TAGS[(_lumen_get_tag_name(desc[i]) || '').toUpperCase()] === 1) {
                return _lumen_make_element(desc[i]);
            }
        }
        return null;
    },
    enumerable: true, configurable: true,
});

// ── <select> / <option> (HTML LS §4.10.7, §4.10.10) ──────────────────────────

// The flattened list of options of a <select>: direct <option> children plus
// the ones nested in <optgroup>.
function _lumen_select_options(select_nid) {
    var out = [];
    var kids = _lumen_get_children(select_nid);
    for (var i = 0; i < kids.length; i++) {
        var tag = (_lumen_get_tag_name(kids[i]) || '').toUpperCase();
        if (tag === 'OPTION') out.push(kids[i]);
        else if (tag === 'OPTGROUP') {
            var sub = _lumen_get_children(kids[i]);
            for (var j = 0; j < sub.length; j++) {
                if ((_lumen_get_tag_name(sub[j]) || '').toUpperCase() === 'OPTION') out.push(sub[j]);
            }
        }
    }
    return out;
}
function _lumen_option_owner_select(option_nid) {
    var cur = _lumen_u2n(_lumen_get_parent(option_nid));
    for (var guard = 0; guard < 8 && cur !== null; guard++) {
        var tag = (_lumen_get_tag_name(cur) || '').toUpperCase();
        if (tag === 'SELECT') return cur;
        if (tag !== 'OPTGROUP') return -1;
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    return -1;
}
function _lumen_option_text(nid) {
    var t = _lumen_get_text_content(nid);
    return (t === null || t === undefined) ? '' : String(t).trim();
}
function _lumen_option_is_selected(option_nid) {
    if (_lumen_option_selected[option_nid] !== undefined) return _lumen_option_selected[option_nid];
    return _lumen_has_attr(option_nid, 'selected');
}
// HTML LS §4.10.7 «ask for a reset»: a single-selection <select> always has a
// selected option — the first non-disabled one when the markup names none.
function _lumen_select_selected_index(select_nid) {
    var opts = _lumen_select_options(select_nid);
    for (var i = 0; i < opts.length; i++) if (_lumen_option_is_selected(opts[i])) return i;
    if (!_lumen_has_attr(select_nid, 'multiple')) {
        for (var j = 0; j < opts.length; j++) {
            if (!_lumen_has_attr(opts[j], 'disabled')) return j;
        }
    }
    return -1;
}
function _lumen_select_set_index(select_nid, index) {
    var opts = _lumen_select_options(select_nid);
    for (var i = 0; i < opts.length; i++) _lumen_option_selected[opts[i]] = (i === index);
    // The shell's own copy of the value is now stale; recompute on next read.
    delete _input_values[select_nid];
}
function _lumen_select_set_value(select_nid, value) {
    var opts = _lumen_select_options(select_nid);
    var match = -1;
    for (var i = 0; i < opts.length; i++) {
        if (_lumen_make_element(opts[i]).value === value) { match = i; break; }
    }
    _lumen_select_set_index(select_nid, match);
    if (match === -1) _input_values[select_nid] = '';
}

Object.defineProperty(HTMLSelectElement.prototype, 'options', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return null;
        return _lumen_make_nid_collection(function() { return _lumen_select_options(n); },
                                          HTMLOptionsCollection.prototype);
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLSelectElement.prototype, 'selectedOptions', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return null;
        return _lumen_make_nid_collection(function() {
            return _lumen_select_options(n).filter(_lumen_option_is_selected);
        }, HTMLCollection.prototype);
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLSelectElement.prototype, 'length', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        return n === -1 ? 0 : _lumen_select_options(n).length;
    },
    set: function(v) {
        // Truncating via `select.length = N` removes trailing options; growing
        // it is a no-op here (it would require minting bare <option>s).
        var n = _lumen_reflect_nid(this);
        if (n === -1) return;
        var opts = _lumen_select_options(n);
        var want = Number(v); if (!isFinite(want) || want < 0) want = 0;
        for (var i = opts.length - 1; i >= want; i--) _lumen_make_element(opts[i]).remove();
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLSelectElement.prototype, 'selectedIndex', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return -1;
        // A shell-driven pick lands in `_input_values`; map it back to an index
        // so `selectedIndex` and `value` cannot disagree.
        if (_input_values[n] !== undefined) {
            var opts = _lumen_select_options(n);
            for (var i = 0; i < opts.length; i++) {
                if (_lumen_make_element(opts[i]).value === _input_values[n]) return i;
            }
            return -1;
        }
        return _lumen_select_selected_index(n);
    },
    set: function(v) {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return;
        var idx = Number(v); if (!isFinite(idx)) idx = -1;
        _lumen_select_set_index(n, Math.trunc(idx));
    },
    enumerable: true, configurable: true,
});
HTMLSelectElement.prototype.item = function(i) {
    var n = _lumen_reflect_nid(this);
    if (n === -1) return null;
    var opts = _lumen_select_options(n);
    i = i >>> 0;
    return i < opts.length ? _lumen_make_element(opts[i]) : null;
};
HTMLSelectElement.prototype.namedItem = function(name) {
    var n = _lumen_reflect_nid(this);
    if (n === -1) return null;
    return _lumen_html_collection_named(_lumen_select_options(n), String(name));
};
// HTML LS §4.10.7: `add(element, before)` — `before` is an option or an index;
// omitted/null appends.
HTMLSelectElement.prototype.add = function(element, before) {
    var n = _lumen_reflect_nid(this);
    if (n === -1 || !element) return;
    if (before === undefined || before === null) { this.appendChild(element); return; }
    var refNid = -1;
    if (typeof before === 'number') {
        var opts = _lumen_select_options(n);
        var bi = Math.trunc(before);
        if (bi >= 0 && bi < opts.length) refNid = opts[bi];
    } else if (before.__nid__ !== undefined) {
        refNid = before.__nid__;
    }
    if (refNid === -1) { this.appendChild(element); return; }
    var parent = _lumen_u2n(_lumen_get_parent(refNid));
    if (parent === null) { this.appendChild(element); return; }
    _lumen_make_element(parent).insertBefore(element, _lumen_make_element(refNid));
};
// `select.remove(index)` (HTML LS §4.10.7) lives in the wrapper's own `remove`,
// next to `ChildNode.remove()` — an own property shadows the prototype, so an
// override here would never run.

Object.defineProperty(HTMLOptionElement.prototype, 'selected', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        return n === -1 ? false : _lumen_option_is_selected(n);
    },
    set: function(v) {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return;
        var sel = _lumen_option_owner_select(n);
        if (v && sel !== -1 && !_lumen_has_attr(sel, 'multiple')) {
            var opts = _lumen_select_options(sel);
            for (var i = 0; i < opts.length; i++) _lumen_option_selected[opts[i]] = (opts[i] === n);
        } else {
            _lumen_option_selected[n] = !!v;
        }
        if (sel !== -1) delete _input_values[sel];
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLOptionElement.prototype, 'index', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return 0;
        var sel = _lumen_option_owner_select(n);
        if (sel === -1) return 0;
        var idx = _lumen_select_options(sel).indexOf(n);
        return idx === -1 ? 0 : idx;
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLOptionElement.prototype, 'text', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        return n === -1 ? '' : _lumen_option_text(n);
    },
    set: function(v) {
        var n = _lumen_reflect_nid(this);
        if (n !== -1) _lumen_set_text_content(n, String(v));
    },
    enumerable: true, configurable: true,
});
// `label` reflects the content attribute but falls back to the option's text.
Object.defineProperty(HTMLOptionElement.prototype, 'label', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return '';
        var v = _lumen_u2n(_lumen_get_attr(n, 'label'));
        return v !== null ? String(v) : _lumen_option_text(n);
    },
    set: function(v) {
        var n = _lumen_reflect_nid(this);
        if (n !== -1) _lumen_set_attr(n, 'label', String(v));
    },
    enumerable: true, configurable: true,
});
// HTML LS §4.10.10 — the legacy `Option(text, value, defaultSelected, selected)`
// factory, the counterpart of `Image()`.
function Option(text, value, defaultSelected, selected) {
    var op = document.createElement('option');
    if (text !== undefined && text !== null && String(text) !== '') op.text = String(text);
    if (value !== undefined && value !== null) op.setAttribute('value', String(value));
    if (defaultSelected) op.setAttribute('selected', '');
    op.selected = !!selected;
    return op;
}
window.Option = Option;

// ── Text selection on <input>/<textarea> (HTML LS §4.10.5.4) ─────────────────
function _lumen_selection_applies(nid) {
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    if (tag === 'TEXTAREA') return true;
    if (tag !== 'INPUT') return false;
    var t = _lumen_u2n(_lumen_get_attr(nid, 'type'));
    t = (t === null) ? 'text' : String(t).toLowerCase();
    return _LUMEN_SELECTABLE_INPUT_TYPES[t] === 1;
}
function _lumen_selection_of(nid) {
    var s = _lumen_selection_state[nid];
    if (s === undefined) { s = { start: 0, end: 0, direction: 'none' }; _lumen_selection_state[nid] = s; }
    return s;
}
// Named `_lumen_set_text_selection`, not `…_set_selection`: the latter is the
// native that drives the *document* selection (`window.getSelection()`), and a
// same-named shim function would shadow it for the whole page.
function _lumen_set_text_selection(nid, start, end, direction) {
    var len = String(_lumen_make_element(nid).value || '').length;
    start = Math.trunc(Number(start)); if (!isFinite(start) || start < 0) start = 0;
    end   = Math.trunc(Number(end));   if (!isFinite(end)   || end   < 0) end   = 0;
    if (start > len) start = len;
    if (end > len) end = len;
    if (end < start) start = end;
    var dir = (direction === 'forward' || direction === 'backward') ? direction : 'none';
    _lumen_selection_state[nid] = { start: start, end: end, direction: dir };
    // §4.10.5.4 step 4: queue a `select` event when the selection actually moves.
    _lumen_dispatch_rich(nid, new Event('select', { bubbles: true, cancelable: false }));
}
[HTMLInputElement.prototype, HTMLTextAreaElement.prototype].forEach(function(_p) {
    ['selectionStart', 'selectionEnd'].forEach(function(_prop) {
        var key = (_prop === 'selectionStart') ? 'start' : 'end';
        Object.defineProperty(_p, _prop, {
            get: function() {
                var n = _lumen_reflect_nid(this);
                if (n === -1 || !_lumen_selection_applies(n)) return null;
                return _lumen_selection_of(n)[key];
            },
            set: function(v) {
                var n = _lumen_reflect_nid(this);
                if (n === -1 || !_lumen_selection_applies(n)) return;
                var s = _lumen_selection_of(n);
                if (key === 'start') _lumen_set_text_selection(n, v, Math.max(Number(v) || 0, s.end), s.direction);
                else _lumen_set_text_selection(n, s.start, v, s.direction);
            },
            enumerable: true, configurable: true,
        });
    });
    Object.defineProperty(_p, 'selectionDirection', {
        get: function() {
            var n = _lumen_reflect_nid(this);
            if (n === -1 || !_lumen_selection_applies(n)) return null;
            return _lumen_selection_of(n).direction;
        },
        set: function(v) {
            var n = _lumen_reflect_nid(this);
            if (n === -1 || !_lumen_selection_applies(n)) return;
            var s = _lumen_selection_of(n);
            _lumen_set_text_selection(n, s.start, s.end, String(v));
        },
        enumerable: true, configurable: true,
    });
    _p.setSelectionRange = function(start, end, direction) {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return;
        if (!_lumen_selection_applies(n)) {
            throw new DOMException(
                'setSelectionRange does not apply to this input type', 'InvalidStateError');
        }
        _lumen_set_text_selection(n, start, end, direction);
    };
    _p.select = function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return;
        var len = String(this.value || '').length;
        _lumen_set_text_selection(n, 0, len, 'none');
    };
    _p.setRangeText = function(replacement, start, end, mode) {
        var n = _lumen_reflect_nid(this);
        if (n === -1 || !_lumen_selection_applies(n)) return;
        var s = _lumen_selection_of(n);
        var from = (start === undefined) ? s.start : Math.trunc(Number(start));
        var to   = (end   === undefined) ? s.end   : Math.trunc(Number(end));
        var val = String(this.value || '');
        if (from > to) { var t = from; from = to; to = t; }
        var repl = String(replacement);
        this.value = val.slice(0, from) + repl + val.slice(to);
        var newEnd = from + repl.length;
        if (mode === 'start') _lumen_set_text_selection(n, from, from, 'none');
        else if (mode === 'end') _lumen_set_text_selection(n, newEnd, newEnd, 'none');
        else _lumen_set_text_selection(n, from, newEnd, 'none');
    };
});

// ── Remaining HTMLInputElement bits that are state, not reflection ───────────
Object.defineProperty(HTMLInputElement.prototype, 'indeterminate', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        return n === -1 ? false : !!_lumen_indeterminate[n];
    },
    set: function(v) {
        var n = _lumen_reflect_nid(this);
        if (n !== -1) _lumen_indeterminate[n] = !!v;
    },
    enumerable: true, configurable: true,
});
// `files` is `null` for every type but `file`. Lumen has no file picker yet, so
// a file input reports an empty FileList rather than a fabricated one.
Object.defineProperty(HTMLInputElement.prototype, 'files', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return null;
        var t = _lumen_u2n(_lumen_get_attr(n, 'type'));
        if (t === null || String(t).toLowerCase() !== 'file') return null;
        var empty = { length: 0, item: function() { return null; } };
        return empty;
    },
    enumerable: true, configurable: true,
});
// `list` points at the <datalist> named by the `list` attribute (HTML LS §4.10.5).
Object.defineProperty(HTMLInputElement.prototype, 'list', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        if (n === -1) return null;
        var ref = _lumen_u2n(_lumen_get_attr(n, 'list'));
        if (ref === null || String(ref) === '') return null;
        var el = _lumen_u2n(_lumen_get_element_by_id(String(ref)));
        if (el === null) return null;
        return (_lumen_get_tag_name(el) || '').toUpperCase() === 'DATALIST'
            ? _lumen_make_element(el) : null;
    },
    enumerable: true, configurable: true,
});
// `defaultValue` on <textarea> is its child text, not a content attribute
// (HTML LS §4.10.11) — so it cannot be a reflection-table row.
Object.defineProperty(HTMLTextAreaElement.prototype, 'defaultValue', {
    get: function() {
        var n = _lumen_reflect_nid(this);
        return n === -1 ? '' : _lumen_get_text_content(n);
    },
    set: function(v) {
        var n = _lumen_reflect_nid(this);
        if (n !== -1) _lumen_set_text_content(n, String(v));
    },
    enumerable: true, configurable: true,
});
Object.defineProperty(HTMLTextAreaElement.prototype, 'textLength', {
    get: function() { return String(this.value || '').length; },
    enumerable: true, configurable: true,
});
// HTML LS §4.10.5.4 — stepUp/stepDown for the numeric input types.
[['stepUp', 1], ['stepDown', -1]].forEach(function(_e) {
    HTMLInputElement.prototype[_e[0]] = function(n) {
        var nid = _lumen_reflect_nid(this);
        if (nid === -1) return;
        var stepAttr = _lumen_u2n(_lumen_get_attr(nid, 'step'));
        var step = (stepAttr === null || String(stepAttr) === 'any') ? 1 : Number(stepAttr);
        if (!isFinite(step) || step <= 0) step = 1;
        var count = (n === undefined) ? 1 : Number(n);
        if (!isFinite(count)) count = 1;
        var cur = Number(this.value);
        if (!isFinite(cur)) cur = 0;
        var next = cur + _e[1] * step * count;
        var minA = _lumen_u2n(_lumen_get_attr(nid, 'min'));
        var maxA = _lumen_u2n(_lumen_get_attr(nid, 'max'));
        if (minA !== null && isFinite(Number(minA)) && next < Number(minA)) next = Number(minA);
        if (maxA !== null && isFinite(Number(maxA)) && next > Number(maxA)) next = Number(maxA);
        // Trim binary-float noise (0.1 + 0.2) without pulling in a formatter.
        this.value = String(Math.round(next * 1e9) / 1e9);
    };
});

// ── Activation: HTMLElement.click() (HTML LS §4.10.19 / DOM §3.4) ────────────
// The only standard way to press a control from script, and the only way to
// start a download through a synthetic `<a download>`. Runs the full sequence:
// pre-click activation (checkbox/radio flip) → dispatch a cancelable `click` →
// activation behaviour, or undo the pre-click flip when a handler cancelled.
function _lumen_pre_click_activation(nid) {
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    if (tag !== 'INPUT') return null;
    var t = _lumen_u2n(_lumen_get_attr(nid, 'type'));
    t = (t === null) ? 'text' : String(t).toLowerCase();
    var el = _lumen_make_element(nid);
    if (t === 'checkbox') {
        var was = el.checked;
        _lumen_indeterminate[nid] = false;
        el.checked = !was;
        return { kind: 'checkbox', checked: was };
    }
    if (t === 'radio') {
        var wasR = el.checked;
        _lumen_radio_select(nid);
        return { kind: 'radio', checked: wasR };
    }
    return null;
}
function _lumen_undo_pre_click_activation(nid, pre) {
    if (!pre) return;
    _lumen_make_element(nid).checked = pre.checked;
}
// HTML LS §4.10.5.1.13 «radio button group»: same form owner, same name.
// BUG-444: goes through the dirty-checkedness accessors, not the `checked`
// attribute directly — the `checked` getter now reads the dirty flag first,
// so writing the attribute on a sibling that already has one is a no-op.
function _lumen_radio_select(nid) {
    var name = _lumen_u2n(_lumen_get_attr(nid, 'name'));
    var owner = _lumen_form_owner(nid);
    var scope = (owner !== -1) ? owner : _lumen_root_nid;
    var all = _lumen_descendant_elements(scope, []);
    for (var i = 0; i < all.length; i++) {
        if ((_lumen_get_tag_name(all[i]) || '').toUpperCase() !== 'INPUT') continue;
        var t = _lumen_u2n(_lumen_get_attr(all[i], 'type'));
        if (t === null || String(t).toLowerCase() !== 'radio') continue;
        if (name !== null && _lumen_u2n(_lumen_get_attr(all[i], 'name')) !== name) continue;
        if (all[i] !== nid) _lumen_set_dirty_checked(all[i], false);
    }
    _lumen_set_dirty_checked(nid, true);
}
function _lumen_fire_input_and_change(nid) {
    _lumen_dispatch_rich(nid, new Event('input',  { bubbles: true, cancelable: false }));
    _lumen_dispatch_rich(nid, new Event('change', { bubbles: true, cancelable: false }));
}
// Tags that carry an activation behaviour of their own — the table
// `_lumen_run_activation_behavior` below dispatches on, kept separate so the
// activation-target walk can ask «does this node have one» without running it.
var _LUMEN_ACTIVATABLE_TAGS = {
    INPUT: 1, BUTTON: 1, A: 1, AREA: 1, SUMMARY: 1, LABEL: 1,
};
// Interactive content with no activation behaviour of its own. The walk stops
// here rather than continuing to an ancestor: HTML LS §4.10.20 says a label
// does nothing for events targeted at its interactive-content descendants, so
// a click on the `<textarea>` inside `<label for=cb>` must not toggle `cb`.
var _LUMEN_ACTIVATION_BARRIER_TAGS = {
    SELECT: 1, TEXTAREA: 1, IFRAME: 1, EMBED: 1, OBJECT: 1,
};
// DOM Standard §2.9 «activation target»: the activation behaviour belongs to
// the nearest inclusive ancestor of the event target that has one, not to the
// target itself (BUG-837 — a click on the `<span>` inside a `<label>`, the
// `<img>` inside an `<a>` or the `<svg>` inside a `<button>` used to dispatch
// the event and then do nothing, because the tag table was consulted for the
// clicked node alone). The walk is bounded by the event path, i.e. the ancestor
// chain of `nid`. Returns -1 when nothing on the path is activatable.
function _lumen_activation_target(nid) {
    var cur = nid;
    for (var guard = 0; guard < 512; guard++) {
        if (cur === null || cur === undefined || cur === -1) return -1;
        var tag = (_lumen_get_tag_name(cur) || '').toUpperCase();
        // HTML LS §4.6.1: an `<a>`/`<area>` with no `href` is a placeholder, not
        // a hyperlink — it has no activation behaviour and is not interactive
        // content, so the walk must pass straight through it. Stopping there is
        // what made a click on the `<a>` inside a `<summary>` (the shape
        // `anchor-without-link.html` measures) reach the link branch, find no
        // `href` and silently do nothing at all.
        var activatable = _LUMEN_ACTIVATABLE_TAGS[tag] === 1
            && !((tag === 'A' || tag === 'AREA') && !_lumen_has_attr(cur, 'href'));
        if (activatable) return cur;
        if (_LUMEN_ACTIVATION_BARRIER_TAGS[tag] === 1) return -1;
        cur = _lumen_u2n(_lumen_get_parent(cur));
    }
    return -1;
}
// HTML LS §4.11.2 «summary element»: an element is *the* summary for its parent
// details only if it is that parent's FIRST summary child. A second `<summary>`
// is ordinary flow content and activating it must do nothing. Returns the parent
// `<details>` nid, or -1.
function _lumen_summary_details_parent(nid) {
    var parent = _lumen_u2n(_lumen_get_parent(nid));
    if (parent === null || (_lumen_get_tag_name(parent) || '').toUpperCase() !== 'DETAILS') return -1;
    var kids = _lumen_get_children(parent);
    for (var i = 0; i < kids.length; i++) {
        if ((_lumen_get_tag_name(kids[i]) || '').toUpperCase() !== 'SUMMARY') continue;
        return kids[i] === nid ? parent : -1;
    }
    return -1;
}
function _lumen_run_activation_behavior(nid, el) {
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    // HTML LS §4.10.19: a disabled form control has no activation behaviour.
    // `click()` checks the node it was called on; now that the target may be an
    // ancestor of it, the disabled ancestor has to be checked here as well —
    // otherwise a click on the `<span>` inside `<button disabled>` would submit.
    if (_LUMEN_DISABLEABLE_TAGS[tag] === 1 && _lumen_has_attr(nid, 'disabled')) return;
    if (tag === 'INPUT') {
        var t = _lumen_u2n(_lumen_get_attr(nid, 'type'));
        t = (t === null) ? 'text' : String(t).toLowerCase();
        if (t === 'checkbox' || t === 'radio') { _lumen_fire_input_and_change(nid); return; }
        if (t === 'submit' || t === 'image') { _lumen_activate_submit(nid); return; }
        if (t === 'reset') { _lumen_activate_reset(nid); return; }
        return;
    }
    if (tag === 'BUTTON') {
        var bt = _lumen_u2n(_lumen_get_attr(nid, 'type'));
        bt = (bt === null) ? 'submit' : String(bt).toLowerCase();
        if (bt === 'submit') { _lumen_activate_submit(nid); return; }
        if (bt === 'reset') { _lumen_activate_reset(nid); return; }
        return;
    }
    if (tag === 'A' || tag === 'AREA') {
        // HTML LS §7.4.2 picks «navigate to a fragment» or «load a document» by
        // comparing URLs, not by how the navigation was started, so activating a
        // hyperlink takes exactly the same entry point as `location.href =`.
        // This used to call `_lumen_navigate` unconditionally (BUG-833), so a
        // click on a fragment-only link reloaded the whole document: no
        // `hashchange`, `location` back without the fragment, and a page that
        // clicks such a link from script looping through reloads forever.
        var href = el.href;
        if (href) _lumen_navigate_or_fragment(String(href), false);
        return;
    }
    if (tag === 'SUMMARY') {
        // HTML LS §4.11.2: the activation behaviour belongs to a summary that is
        // *the* summary for its parent details — the first summary child — and to
        // no other. The flip goes through the ordinary attribute writes so the
        // §4.11.1 change steps run: they, not this branch, own the `toggle` event
        // and the exclusive-accordion pass (BUG-851).
        var parent = _lumen_summary_details_parent(nid);
        if (parent !== -1) {
            if (_lumen_has_attr(parent, 'open')) _lumen_remove_attr(parent, 'open');
            else _lumen_set_attr(parent, 'open', '');
        }
        return;
    }
    if (tag === 'LABEL') {
        var ctrl = el.control;
        // The re-entrancy guard below keeps label → control → label finite.
        if (ctrl && typeof ctrl.click === 'function') ctrl.click();
    }
}
function _lumen_activate_submit(nid) {
    var owner = _lumen_form_owner(nid);
    if (owner === -1) return;
    _lumen_make_element(owner).requestSubmit(_lumen_make_element(nid));
}
function _lumen_activate_reset(nid) {
    var owner = _lumen_form_owner(nid);
    if (owner !== -1) _lumen_make_element(owner).reset();
}
HTMLElement.prototype.click = function() {
    var nid = _lumen_reflect_nid(this);
    if (nid === -1) return;
    _lumen_perform_click(nid);
};
// Shared body of the untrusted click() sequence (DOM §2.9): disabled check,
// re-entrancy guard, activation-target computed before dispatch, the dispatch
// itself and the post-dispatch half. Used both by the prototype method above
// and by the BUG-480 slice-6 cross-frame hook (_lumen_deliver_frame_click): a
// parent-initiated facade click() must run THE CHILD'S OWN click semantics in
// the child isolate, not a re-implementation of them.
function _lumen_perform_click(nid) {
    var tag = (_lumen_get_tag_name(nid) || '').toUpperCase();
    // HTML LS §4.10.19: a disabled form control is not activated at all — no
    // event either.
    if (_LUMEN_DISABLEABLE_TAGS[tag] === 1 && _lumen_has_attr(nid, 'disabled')) return;
    if (_lumen_click_in_progress[nid]) return;
    _lumen_click_in_progress[nid] = true;
    try {
        // DOM §2.9: the activation target is computed *before* dispatch, and
        // both halves of the sequence belong to it — the pre-click flip as much
        // as the behaviour itself (BUG-837).
        var at = _lumen_activation_target(nid);
        var pre = (at !== -1) ? _lumen_pre_click_activation(at) : null;
        var ev = new MouseEvent('click', {
            bubbles: true, cancelable: true, composed: true, isTrusted: false,
            clientX: 0, clientY: 0, screenX: 0, screenY: 0,
            button: 0, buttons: 0, detail: 1,
        });
        var notCancelled = _lumen_dispatch_rich(nid, ev);
        if (at !== -1) {
            if (notCancelled) {
                _lumen_run_activation_behavior(at, (at === nid) ? _lumen_make_element(nid) : _lumen_make_element(at));
            } else {
                _lumen_undo_pre_click_activation(at, pre);
            }
        }
    } finally {
        delete _lumen_click_in_progress[nid];
    }
}

// ── form.submit() / requestSubmit() / reset() (HTML LS §4.10.21) ─────────────
// `reset()` is entirely a document-side operation and runs here. Submission is
// not: encoding, enctype and the actual navigation live in the shell
// (`forms.rs`), so the page-initiated form goes to it through the same
// queue-and-drain shape `focus()` uses (`_lumen_request_form_submit`).
HTMLFormElement.prototype.reset = function() {
    var nid = _lumen_reflect_nid(this);
    if (nid === -1) return;
    var ev = new Event('reset', { bubbles: true, cancelable: true });
    if (!_lumen_dispatch_rich(nid, ev)) return;
    var controls = _lumen_listed_controls(nid);
    for (var i = 0; i < controls.length; i++) {
        var cn = controls[i];
        var tag = (_lumen_get_tag_name(cn) || '').toUpperCase();
        if (tag === 'SELECT') {
            var opts = _lumen_select_options(cn);
            for (var j = 0; j < opts.length; j++) delete _lumen_option_selected[opts[j]];
            delete _input_values[cn];
            continue;
        }
        if (tag !== 'INPUT' && tag !== 'TEXTAREA') continue;
        // Dropping the dirty value restores `value` to its default, which the
        // getter derives from the content attribute (or the child text).
        // The document-side store is the one layout and submission read
        // (BUG-441); `_input_values` is cleared too for controls that predate
        // the wrapper, so nothing stale survives the reset.
        _lumen_clear_dirty_value(cn);
        delete _input_values[cn];
        delete _lumen_selection_state[cn];
        delete _lumen_indeterminate[cn];
        // Dropping the dirty checkedness restores `checked` to its default —
        // the `checked` content attribute (BUG-444, same shape as the dirty
        // value clear above).
        if (tag === 'INPUT') _lumen_clear_dirty_checked(cn);
    }
};
HTMLFormElement.prototype.requestSubmit = function(submitter) {
    var nid = _lumen_reflect_nid(this);
    if (nid === -1) return;
    var submitterNid = -1;
    if (submitter !== undefined && submitter !== null) {
        if (submitter.__nid__ === undefined) {
            throw new TypeError('requestSubmit: the submitter is not an element');
        }
        if (_lumen_form_owner(submitter.__nid__) !== nid) {
            throw new DOMException(
                'The submitter is not owned by this form', 'NotFoundError');
        }
        submitterNid = submitter.__nid__;
    }
    // §4.10.21.4 step 11: fire a cancelable `submit` before handing over.
    if (!_lumen_dispatch_submit_event(nid, submitterNid)) return;
    _lumen_request_form_submit(nid, submitterNid);
};
// §4.10.21.3: `submit()` skips both constraint validation and the `submit`
// event — that is the whole difference from `requestSubmit()`.
HTMLFormElement.prototype.submit = function() {
    var nid = _lumen_reflect_nid(this);
    if (nid !== -1) _lumen_request_form_submit(nid, -1);
};

// ── <selectlist> helpers (Open UI Customizable Select §3) ─────────────────────
// Returns the <listbox> child nid of a <selectlist>, or null if absent.
function _lumen_selectlist_listbox(sl_nid) {
    var kids = _lumen_get_children(sl_nid);
    for (var i = 0; i < kids.length; i++) {
        if ((_lumen_get_tag_name(kids[i]) || '').toLowerCase() === 'listbox') return kids[i];
    }
    return null;
}

// Returns an array of element objects for all <option> children of a
// <selectlist> — either direct children or inside a <listbox> child.
function _lumen_selectlist_options(sl_nid) {
    var out = [];
    var kids = _lumen_get_children(sl_nid);
    for (var i = 0; i < kids.length; i++) {
        var tag = (_lumen_get_tag_name(kids[i]) || '').toLowerCase();
        if (tag === 'option') {
            out.push(_lumen_make_element(kids[i]));
        } else if (tag === 'listbox') {
            var gkids = _lumen_get_children(kids[i]);
            for (var j = 0; j < gkids.length; j++) {
                if ((_lumen_get_tag_name(gkids[j]) || '').toLowerCase() === 'option') {
                    out.push(_lumen_make_element(gkids[j]));
                }
            }
        }
    }
    return out;
}

// ── <details> (HTML LS §4.11.1) ──────────────────────────────────────────────
// `open` *is* the element's state, so everything that can change it — the
// `<summary>` activation behaviour, `d.open = …`, `setAttribute`/`removeAttribute`
// /`toggleAttribute`, the parser's own markup and the shell's native mouse click
// — funnels through the attribute change steps below, and nothing else touches
// the attribute on its own. BUG-851: the flip used to live in two places at once
// (a `click` listener on `document`, which also dispatched `toggle`, and the
// activation behaviour), so a scripted `summary.click()` flipped `open` twice and
// landed back where it started — the handler saw `open`, the next statement saw
// the attribute gone — while a script *write* to `open` notified nobody at all.

// nid -> { oldState, newState } for a toggle task that is queued but has not run.
// HTML LS «queue a details toggle event task» reuses a pending task's `oldState`
// and removes the queued task rather than queueing a second one, so two changes
// inside one turn produce ONE event describing the whole span — which is what
// `toggleEvent.html` t2/t6/t8 assert when they expect `closed` → `closed`.
var _details_toggle_pending = {};
// nid -> true|false: the state this machinery has already accounted for. A
// `<details open>` written by the parser owes an event that nothing queued (the
// shell parses the whole document before the first script runs, so there is no
// insertion point to hook — the BUG-827 shape), and this is what tells the
// end-of-parse scan which elements it still owes one.
var _details_known_open = {};

function _lumen_is_details(nid) {
    var t = _lumen_get_tag_name(nid);
    return t !== null && t !== undefined && String(t).toLowerCase() === 'details';
}

function _lumen_details_fire_toggle(nid) {
    var rec = _details_toggle_pending[nid];
    if (!rec) return;
    delete _details_toggle_pending[nid];
    var evt = new ToggleEvent('toggle', {
        bubbles: false, cancelable: false, isTrusted: true,
        oldState: rec.oldState, newState: rec.newState
    });
    // `_lumen_dispatch` sets no target of its own (BUG-873), and a page that
    // arms one listener over several `<details>` has nothing else to tell them
    // apart — `name-attribute.html` asserts `event.target === element` on every
    // one of the four elements it watches.
    evt.target = _lumen_make_element(nid);
    _lumen_dispatch(nid, evt);
}

// HTML LS §4.11.1 «queue a details toggle event task». Written straight into
// `_lumen_timers` with `nesting: 0` rather than through `setTimeout`, for the
// reason `_ro_schedule_initial`/`_lumen_fire_hashchange` give: the §8.6 4 ms
// clamp is about timer *nesting* and must not apply to a task the engine queues
// on the page's behalf.
function _lumen_details_queue_toggle(nid, oldState, newState) {
    var pending = _details_toggle_pending[nid];
    if (pending) {
        // The queued task has not run yet, so it reports the span from where the
        // element was when it was queued to where it is now. No second task.
        pending.newState = newState;
        return;
    }
    _details_toggle_pending[nid] = { oldState: oldState, newState: newState };
    var deadline = (typeof _lumen_now_ms === 'function') ? _lumen_now_ms() : 0;
    _lumen_timers.push({
        id: _lumen_timer_seq++,
        fn: function() { _lumen_details_fire_toggle(nid); },
        deadline: deadline, interval: null, nesting: 0
    });
    if (typeof _lumen_request_wakeup === 'function') _lumen_request_wakeup(deadline);
}

// HTML LS §4.11.1.1 «ensure details exclusivity by closing the given element if
// needed»: opening a `<details name=X>` closes every *other* `<details name=X>`
// in the same tree. Each of those goes back through the change steps below, so
// they queue their own `toggle` — the spec's own shape, not a special case.
function _lumen_details_ensure_exclusivity(nid) {
    var name = _lumen_u2n(_lumen_get_attr(nid, 'name'));
    if (name === null || String(name) === '') return;
    var all;
    try { all = document.getElementsByTagName('details'); } catch (e) { return; }
    if (!all) return;
    for (var i = 0; i < all.length; i++) {
        var other = all[i];
        if (!other || other.__nid__ === undefined || other.__nid__ === nid) continue;
        if (_lumen_get_attr(other.__nid__, 'open') === undefined) continue;
        var oname = _lumen_u2n(_lumen_get_attr(other.__nid__, 'name'));
        if (oname === null || String(oname) !== String(name)) continue;
        _lumen_remove_attr(other.__nid__, 'open');
    }
}

// HTML LS §4.11.1 attribute change steps for `open`. `wasOpen`/`isOpen` are the
// *presence* of the attribute before and after the write: rewriting its value
// (`open=''` → `open='open'`) is not a state change and owes no event, which is
// what keeps `details9.open = true` on an already-open element silent.
function _lumen_details_open_changed(nid, wasOpen, isOpen) {
    wasOpen = !!wasOpen; isOpen = !!isOpen;
    _details_known_open[nid] = isOpen;
    if (wasOpen === isOpen) return;
    if (isOpen) _lumen_details_ensure_exclusivity(nid);
    _lumen_details_queue_toggle(nid, wasOpen ? 'open' : 'closed', isOpen ? 'open' : 'closed');
}

// The single point where a change to `open` becomes an event. Wrapping the two
// natives (the `_mo_notify` shape further up this file) is what makes the `open`
// property, `setAttribute`, `removeAttribute`, `toggleAttribute` and the
// `<summary>` activation behaviour one mechanism instead of five: none of them
// reaches the document without passing through here.
var _orig_set_attr_details = _lumen_set_attr;
_lumen_set_attr = function(nid, name, value) {
    if (String(name) !== 'open' || !_lumen_is_details(nid)) {
        _orig_set_attr_details(nid, name, value);
        return;
    }
    var was = _lumen_get_attr(nid, 'open') !== undefined;
    _orig_set_attr_details(nid, name, value);
    _lumen_details_open_changed(nid, was, true);
};
var _orig_remove_attr_details = _lumen_remove_attr;
_lumen_remove_attr = function(nid, name) {
    if (String(name) !== 'open' || !_lumen_is_details(nid)) {
        _orig_remove_attr_details(nid, name);
        return;
    }
    var was = _lumen_get_attr(nid, 'open') !== undefined;
    _orig_remove_attr_details(nid, name);
    _lumen_details_open_changed(nid, was, false);
};

// The parser's half, called when parsing ends (`_lumen_apply_ready_state`), for
// the same reason `_lumen_link_hints_scan`/`_lumen_script_empty_src_scan` are:
// markup never passes through the hook above, so a `<details open>` the parser
// wrote is owed a `closed` → `open` event that nothing queued. An element a
// script has already moved carries a `_details_known_open` entry and is skipped,
// so it can never report twice.
function _lumen_details_open_scan() {
    var all;
    try { all = document.getElementsByTagName('details'); } catch (e) { return; }
    if (!all) return;
    for (var i = 0; i < all.length; i++) {
        var el = all[i];
        if (!el || el.__nid__ === undefined) continue;
        var nid = el.__nid__;
        if (_details_known_open[nid] !== undefined) continue;
        if (_lumen_get_attr(nid, 'open') === undefined) { _details_known_open[nid] = false; continue; }
        _lumen_details_open_changed(nid, false, true);
    }
}

// Called by the shell (`main.rs`, `FormClickAction::ToggleDetails`) after it has
// flipped `open` itself on a native mouse click. The flip stays on the shell
// side so the attribute is in the document before the relayout that follows it;
// this only runs the change steps the shell cannot run. Before BUG-851 the shell
// dispatched a bare `Event('toggle')` of its own here while the deleted document
// listener flipped the attribute a second time, so a real click on a `<summary>`
// left `<details>` exactly as it found it and fired two events about it.
function _lumen_details_native_toggled(nid, wasOpen) {
    if (!_lumen_is_details(nid)) return;
    _lumen_details_open_changed(nid, !!wasOpen, _lumen_get_attr(nid, 'open') !== undefined);
}

// ── <dialog> Escape key handler (HTML5 §4.11.7) ──────────────────────────────
// Pressing Escape closes the topmost modal dialog: fires `cancel` (cancelable);
// if not prevented, removes `open` and fires `close`.
document.addEventListener('keydown', function(evt) {
    if (evt.key !== 'Escape') return;
    while (_lumen_modal_dialog_nids.length > 0 &&
           _lumen_get_attr(_lumen_modal_dialog_nids[_lumen_modal_dialog_nids.length - 1], 'open') === undefined) {
        _lumen_modal_dialog_nids.pop();
    }
    if (_lumen_modal_dialog_nids.length === 0) return;
    var lastNid = _lumen_modal_dialog_nids[_lumen_modal_dialog_nids.length - 1];
    var cancelEvt = new Event('cancel', { bubbles: false, cancelable: true });
    var notPrevented = _lumen_dispatch(lastNid, cancelEvt);
    if (notPrevented) {
        _lumen_remove_attr(lastNid, 'open');
        _lumen_remove_attr(lastNid, 'data-lumen-modal');
        _lumen_modal_dialog_nids.pop();
        var closeEvt = new Event('close', { bubbles: false, cancelable: false });
        _lumen_dispatch(lastNid, closeEvt);
    }
});

// ── HTML Popover API (WHATWG HTML §6.12) ─────────────────────────────────────
// Top-layer emulation: position:fixed + z-index:2147483647 when open.
// Elements with [popover] are hidden by layout (is_closed_popover in box_tree.rs)
// until showPopover() sets data-lumen-popover-open. Auto-popovers close each
// other and on outside clicks; Escape closes the topmost auto-popover.

// Open auto-popovers in stack order (newest = last).
// Stack of open auto popovers (popover='' or popover='auto').
var _lumen_popover_stack = [];
// Stack of open hint popovers (popover='hint', Popover API Level 2).
// Hints live above autos but are closed when any auto closes.
var _lumen_hint_stack = [];

// Sentinel attribute written by showPopover() — read by layout's is_closed_popover.
var _LPOP_ATTR = 'data-lumen-popover-open';

// Fixed-position styles applied to open popovers (top-layer emulation).
var _LPOP_STYLE = 'position:fixed;z-index:2147483647;inset:auto;margin:auto;overflow:auto;';

function _lumen_popover_show(nid) {
    if (_lumen_get_attr(nid, 'popover') === undefined) {
        throw new DOMException('Element is not a popover', 'NotSupportedError');
    }
    if (_lumen_get_attr(nid, _LPOP_ATTR) !== undefined) return; // already open
    // Popover API §3.5: a popover's `beforetoggle` is cancelable, and cancelling
    // it aborts the show. It used to be dispatched with `cancelable: false`, so
    // `preventDefault()` in a handler did nothing at all (BUG-578).
    var beforeEvt = new ToggleEvent('beforetoggle', {
        bubbles: false, cancelable: true, oldState: 'closed', newState: 'open' });
    beforeEvt.target = _lumen_make_element(nid);
    if (!_lumen_dispatch(nid, beforeEvt)) return;
    // Re-check: still not open? (beforetoggle could in theory trigger re-entrant show)
    if (_lumen_get_attr(nid, _LPOP_ATTR) !== undefined) return;
    var popVal = (_lumen_get_attr(nid, 'popover') || '').toLowerCase();
    var isHint = popVal === 'hint';
    var isAuto = !isHint && popVal !== 'manual';
    if (isHint) {
        // Popover API Level 2 §3.2: showing a hint closes other hints but NOT autos.
        var hs = _lumen_hint_stack.slice();
        for (var hi = hs.length - 1; hi >= 0; hi--) { _lumen_popover_hide(hs[hi]); }
        _lumen_hint_stack.push(nid);
    } else if (isAuto) {
        // Showing an auto popover closes all hints first, then all autos.
        var hs2 = _lumen_hint_stack.slice();
        for (var hi2 = hs2.length - 1; hi2 >= 0; hi2--) { _lumen_popover_hide(hs2[hi2]); }
        var snap = _lumen_popover_stack.slice();
        for (var i = snap.length - 1; i >= 0; i--) { _lumen_popover_hide(snap[i]); }
        _lumen_popover_stack.push(nid);
    }
    _lumen_set_attr(nid, _LPOP_ATTR, '');
    // Apply top-layer emulation via inline style (saved/restored around the forced override).
    var saved = _lumen_get_attr(nid, 'style') !== undefined ? _lumen_get_attr(nid, 'style') : '';
    _lumen_set_attr(nid, 'data-lumen-popover-saved-style', saved);
    // hints get a slightly lower z-index than auto (still above page content).
    var style = isHint ? 'position:fixed;z-index:2147483646;inset:auto;margin:auto;overflow:auto;' : _LPOP_STYLE;
    _lumen_set_attr(nid, 'style', style + (saved ? saved : ''));
    var toggleEvt = new ToggleEvent('toggle', {
        bubbles: false, cancelable: false, oldState: 'closed', newState: 'open' });
    toggleEvt.target = _lumen_make_element(nid);
    _lumen_dispatch(nid, toggleEvt);
}

function _lumen_popover_hide(nid) {
    if (_lumen_get_attr(nid, _LPOP_ATTR) === undefined) return; // already closed
    var beforeEvt = new ToggleEvent('beforetoggle', {
        bubbles: false, cancelable: true, oldState: 'open', newState: 'closed' });
    beforeEvt.target = _lumen_make_element(nid);
    if (!_lumen_dispatch(nid, beforeEvt)) return;
    if (_lumen_get_attr(nid, _LPOP_ATTR) === undefined) return; // closed by beforetoggle re-entry
    // Remove from whichever stack holds this popover.
    var idx = _lumen_popover_stack.indexOf(nid);
    if (idx >= 0) {
        _lumen_popover_stack.splice(idx, 1);
        // Hiding an auto popover also closes all hints above it in the stack.
        var hs3 = _lumen_hint_stack.slice();
        for (var hi3 = hs3.length - 1; hi3 >= 0; hi3--) { _lumen_popover_hide(hs3[hi3]); }
    }
    var hidx = _lumen_hint_stack.indexOf(nid);
    if (hidx >= 0) _lumen_hint_stack.splice(hidx, 1);
    _lumen_remove_attr(nid, _LPOP_ATTR);
    // Restore saved inline style (remove popover-injected portion).
    var saved = _lumen_u2n(_lumen_get_attr(nid, 'data-lumen-popover-saved-style'));
    if (saved !== null) {
        if (saved === '') { _lumen_remove_attr(nid, 'style'); }
        else { _lumen_set_attr(nid, 'style', saved); }
        _lumen_remove_attr(nid, 'data-lumen-popover-saved-style');
    }
    var toggleEvt = new ToggleEvent('toggle', {
        bubbles: false, cancelable: false, oldState: 'open', newState: 'closed' });
    toggleEvt.target = _lumen_make_element(nid);
    _lumen_dispatch(nid, toggleEvt);
}

function _lumen_popover_toggle(nid, force) {
    var isOpen = _lumen_get_attr(nid, _LPOP_ATTR) !== undefined;
    if (force === true || (!isOpen && force === undefined)) {
        _lumen_popover_show(nid);
    } else if (force === false || (isOpen && force === undefined)) {
        _lumen_popover_hide(nid);
    }
}

// Click outside handler — close auto and hint popovers when click lands outside all of them.
// Runs in capture phase so it fires before target-specific handlers.
document.addEventListener('click', function(evt) {
    if (_lumen_popover_stack.length === 0 && _lumen_hint_stack.length === 0) return;
    // Walk from target toward root; if any open popover contains the target, bail.
    var cur = evt.target;
    while (cur && cur.__nid__ !== undefined) {
        if (_lumen_get_attr(cur.__nid__, _LPOP_ATTR) !== undefined) return;
        cur = cur.parentElement;
    }
    // Outside click — close hints first (top-down), then autos (top-down).
    var hs = _lumen_hint_stack.slice();
    for (var hi = hs.length - 1; hi >= 0; hi--) { _lumen_popover_hide(hs[hi]); }
    var snap = _lumen_popover_stack.slice();
    for (var i = snap.length - 1; i >= 0; i--) { _lumen_popover_hide(snap[i]); }
}, true);

// Escape key — close topmost hint or auto-popover (if no modal dialog takes precedence).
document.addEventListener('keydown', function(evt) {
    if (evt.key !== 'Escape') return;
    // Let dialog Escape handler take priority when a modal dialog is open.
    if (_lumen_modal_dialog_nids.length > 0) return;
    // Hints sit on top — close topmost hint first if any.
    if (_lumen_hint_stack.length > 0) {
        _lumen_popover_hide(_lumen_hint_stack[_lumen_hint_stack.length - 1]);
        return;
    }
    if (_lumen_popover_stack.length > 0) {
        _lumen_popover_hide(_lumen_popover_stack[_lumen_popover_stack.length - 1]);
    }
});

// popovertarget / popovertargetaction: button/input clicks trigger show/hide/toggle on target.
document.addEventListener('click', function(evt) {
    var el = evt.target;
    while (el && el.__nid__ !== undefined) {
        var ptId = _lumen_u2n(_lumen_get_attr(el.__nid__, 'popovertarget'));
        if (ptId !== null) {
            var targetNid = _lumen_u2n(_lumen_get_element_by_id(ptId));
            if (targetNid !== null) {
                var action = (_lumen_u2n(_lumen_get_attr(el.__nid__, 'popovertargetaction')) || 'toggle').toLowerCase();
                if (action === 'show')   { _lumen_popover_show(targetNid);              return; }
                if (action === 'hide')   { _lumen_popover_hide(targetNid);              return; }
                /* toggle */ _lumen_popover_toggle(targetNid, undefined); return;
            }
        }
        el = el.parentElement;
    }
});

// ── Fullscreen API helpers ────────────────────────────────────────────────────
// WHATWG Fullscreen §4.3 — the error preconditions of requestFullscreen(),
// evaluated in spec order. Returns null when the request may proceed, or a short
// reason for the TypeError otherwise (BUG-390).
var _LUMEN_FS_NS_HTML   = 'http://www.w3.org/1999/xhtml';
var _LUMEN_FS_NS_SVG    = 'http://www.w3.org/2000/svg';
var _LUMEN_FS_NS_MATHML = 'http://www.w3.org/1998/Math/MathML';

function _lumen_fs_request_error(nid, el) {
    // This is connected: a detached element can never be shown.
    if (!_lumen_resource_is_connected(nid)) return 'element is not connected';
    // This is an HTML element, or an svg / math element.
    var ns = el.namespaceURI;
    var local = String(el.localName || '').toLowerCase();
    if (ns && ns !== _LUMEN_FS_NS_HTML
        && !(ns === _LUMEN_FS_NS_SVG && local === 'svg')
        && !(ns === _LUMEN_FS_NS_MATHML && local === 'math')) {
        return 'element is neither an HTML element nor svg / math';
    }
    // This node document is fullscreen enabled.
    if (!document.fullscreenEnabled) return 'the document is not fullscreen enabled';
    // This popover visibility state is hidden: a showing popover and the
    // fullscreen element are competing occupants of the top layer.
    if (_lumen_get_attr(nid, _LPOP_ATTR) !== undefined) return 'element is a showing popover';
    // Transient activation, or the algorithm is triggered by user generated
    // orchestration. navigator.userActivation is the engine single answer to
    // that question (the FSA and local-font gates read the same property); it
    // currently reports active unconditionally, so this branch only starts
    // firing once the engine tracks real gestures - BUG-758.
    var activation = (typeof navigator !== 'undefined') ? navigator.userActivation : undefined;
    if (activation && activation.isActive === false) return 'no transient user activation';
    return null;
}

// WHATWG Fullscreen §4.3 error path: fire fullscreenerror for a request that
// failed the checks above. The event targets the element and bubbles, so a
// document-level listener sees it with target still pointing at the element; a
// disconnected element has no ancestor chain to bubble along, so the event goes
// straight to the document instead. document.onfullscreenerror is a plain
// property of the document object rather than a per-nid entry in
// _lumen_on_handlers, so neither dispatch path invokes it - call it here.
function _lumen_fire_fullscreen_error(nid) {
    var evt = new Event('fullscreenerror', { bubbles: true, cancelable: false, composed: true });
    if (nid !== null && nid !== undefined && _lumen_resource_is_connected(nid)) {
        _lumen_dispatch_rich(nid, evt);
    } else {
        evt.target = document;
        evt.currentTarget = document;
        document.dispatchEvent(evt);
    }
    if (typeof document.onfullscreenerror === 'function') {
        try { document.onfullscreenerror.call(document, evt); } catch (e) { _lumen_report_exception(e); }
    }
}

// Called by the shell (via eval_js) when fullscreen is exited externally, e.g.
// the user pressed Escape or the OS window manager exited fullscreen mode.
// This keeps JS state consistent with reality — _fs_nid → -1, fires events.
function _lumen_notify_fullscreen_exit() {
    if (_fs_nid !== -1) {
        var old = _fs_nid;
        _lumen_remove_attr(_fs_nid, _FS_ATTR);
        _fs_nid = -1;
        var prev = _lumen_make_element(old);
        if (prev) { prev.dispatchEvent(new Event('fullscreenchange', { bubbles: true })); }
        document.dispatchEvent(new Event('fullscreenchange'));
    }
}

// ── Web Animations API Level 1 (W3C Web Animations §3) ─────────────────────
// Pure JS implementation; ticks via a shared requestAnimationFrame loop.
// P4 wires CSS animation-* properties separately; P2 handles compositor offload.
//
// External API surface (called by _lumen_make_element and document object):
//   _wa_element_animate(target, keyframes, options) → Animation
//   _wa_get_animations_for(target) → Animation[]
//   _wa_doc_get_animations() → Animation[]
//   _wa_doc_timeline — DocumentTimeline singleton

// Current animation timeline time — updated at the start of every RAF tick.
var _wa_current_time = 0;
// Live registry of all non-idle Animation instances.
var _wa_animations = [];

// AnimationPlaybackEvent (W3C Web Animations §4.4.3) — fired on finish/cancel.
function AnimationPlaybackEvent(type, init) {
    Event.call(this, type, { bubbles: false, cancelable: false });
    this.currentTime  = (init && init.currentTime  != null) ? init.currentTime  : null;
    this.timelineTime = (init && init.timelineTime != null) ? init.timelineTime : null;
}
AnimationPlaybackEvent.prototype = Object.create(Event.prototype);
AnimationPlaybackEvent.prototype.constructor = AnimationPlaybackEvent;

// DocumentTimeline — wraps the document's global animation timeline.
function DocumentTimeline(options) {
    this._originTime = (options && options.originTime != null) ? +options.originTime : 0;
}
Object.defineProperty(DocumentTimeline.prototype, 'currentTime', {
    get: function() { return _wa_current_time > 0 ? _wa_current_time - this._originTime : null; },
    configurable: true,
});

// Singleton document timeline — shared across all animations on the page.
var _wa_doc_timeline = new DocumentTimeline();

// Normalize the keyframes argument into a sorted array of
// { offset, easing, composite, <prop>: <value> } objects.
function _wa_normalize_keyframes(keyframes) {
    if (!keyframes) return [];
    var result = [];
    if (Array.isArray(keyframes)) {
        var n = keyframes.length;
        for (var i = 0; i < n; i++) {
            var src = keyframes[i] || {};
            var kf = {};
            kf.offset = (src.offset != null) ? +src.offset : (n <= 1 ? 0 : i / (n - 1));
            kf.easing = src.easing || 'linear';
            kf.composite = src.composite || 'replace';
            for (var p in src) {
                if (p !== 'offset' && p !== 'easing' && p !== 'composite') kf[p] = src[p];
            }
            result.push(kf);
        }
    } else {
        // Property-indexed form: { opacity: [0, 1], transform: ['none', 'rotate(90deg)'] }
        var offsets = Array.isArray(keyframes.offset) ? keyframes.offset : null;
        var len = 0;
        var propNames = [];
        for (var pp in keyframes) {
            if (pp !== 'offset' && pp !== 'easing' && pp !== 'composite' && Array.isArray(keyframes[pp])) {
                if (keyframes[pp].length > len) len = keyframes[pp].length;
                propNames.push(pp);
            }
        }
        for (var j = 0; j < len; j++) {
            var kf2 = {};
            kf2.offset = (offsets && offsets[j] != null) ? +offsets[j] : (len <= 1 ? 0 : j / (len - 1));
            kf2.easing = (Array.isArray(keyframes.easing) ? keyframes.easing[j] : keyframes.easing) || 'linear';
            kf2.composite = 'replace';
            for (var k = 0; k < propNames.length; k++) {
                var arr = keyframes[propNames[k]];
                kf2[propNames[k]] = arr[j];
            }
            result.push(kf2);
        }
    }
    result.sort(function(a, b) { return a.offset - b.offset; });
    return result;
}

// Easing functions: linear / ease / ease-in / ease-out / ease-in-out.
function _wa_ease(t, easing) {
    if (!easing || easing === 'linear') return t;
    if (easing === 'ease-in')  return t * t;
    if (easing === 'ease-out') return t * (2 - t);
    if (easing === 'ease' || easing === 'ease-in-out') return t < 0.5 ? 2*t*t : -1+(4-2*t)*t;
    if (easing === 'step-start') return t > 0 ? 1 : 0;
    if (easing === 'step-end')   return t >= 1 ? 1 : 0;
    // cubic-bezier(p1x, p1y, p2x, p2y) — approximate with de Casteljau.
    var m = easing.match(/^cubic-bezier\(([^,]+),([^,]+),([^,]+),([^)]+)\)$/);
    if (m) {
        var p1x = +m[1], p1y = +m[2], p2x = +m[3], p2y = +m[4];
        // Newton's method to find t_css for x == t, then return y.
        var u = t;
        for (var iter = 0; iter < 8; iter++) {
            var cx = 3*p1x, bx = 3*(p2x-p1x)-cx, ax = 1-cx-bx;
            var x = ((ax*u+bx)*u+cx)*u;
            var dx = (3*ax*u+2*bx)*u+cx;
            if (Math.abs(dx) < 1e-8) break;
            u -= (x - t) / dx;
        }
        var cy = 3*p1y, by = 3*(p2y-p1y)-cy, ay = 1-cy-by;
        return ((ay*u+by)*u+cy)*u;
    }
    return t;
}

// Parse a CSS color string to [r, g, b, a] (0-255).
function _wa_parse_color(str) {
    str = String(str).trim();
    var m;
    if ((m = str.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*([\d.]+))?\s*\)$/))) {
        return [+m[1], +m[2], +m[3], m[4] != null ? Math.round(+m[4]*255) : 255];
    }
    if (str.charAt(0) === '#') {
        var h = str.slice(1);
        if (h.length === 3)  h = h[0]+h[0]+h[1]+h[1]+h[2]+h[2];
        if (h.length === 6)  h += 'ff';
        if (h.length === 8)  return [parseInt(h.slice(0,2),16),parseInt(h.slice(2,4),16),parseInt(h.slice(4,6),16),parseInt(h.slice(6,8),16)];
    }
    return null;
}

// Lerp a CSS color.
function _wa_lerp_color(a, b, t) {
    var ca = _wa_parse_color(a), cb = _wa_parse_color(b);
    if (!ca || !cb) return t < 0.5 ? a : b;
    function lr(x, y) { return Math.round(x + (y-x)*t); }
    var al = lr(ca[3], cb[3]);
    if (al === 255) return 'rgb('+lr(ca[0],cb[0])+','+lr(ca[1],cb[1])+','+lr(ca[2],cb[2])+')';
    return 'rgba('+lr(ca[0],cb[0])+','+lr(ca[1],cb[1])+','+lr(ca[2],cb[2])+','+(al/255).toFixed(4)+')';
}

// Lerp a single CSS scalar+unit value (e.g. '100px', '0.5').
function _wa_lerp_scalar(a, b, t) {
    var na = parseFloat(a), nb = parseFloat(b);
    if (isNaN(na) || isNaN(nb)) return t < 0.5 ? a : b;
    var v = na + (nb - na) * t;
    var ua = String(a).replace(/[0-9. +-]/g, '');
    var ub = String(b).replace(/[0-9. +-]/g, '');
    return v + (ua || ub || '');
}

// CSS color-like property names.
var _wa_color_props = {
    color:1, backgroundColor:1, borderColor:1, outlineColor:1,
    borderTopColor:1, borderRightColor:1, borderBottomColor:1, borderLeftColor:1,
    textDecorationColor:1, fill:1, stroke:1
};

// Parse a transform function string: 'rotate(90deg)' → {name:'rotate', args:['90deg']}.
function _wa_parse_tfn(s) {
    var m = s.match(/^(\w+)\(([^)]*)\)$/);
    return m ? { name: m[1], args: m[2].split(',').map(function(a){ return a.trim(); }) } : null;
}

// Lerp two transform strings using matched-pair lerp when possible.
function _wa_lerp_transform(from, to, t) {
    if (from === to) return from;
    if (from === 'none' && to === 'none') return 'none';
    if (from === 'none') return to;
    if (to === 'none') return from;
    var fns_a = from.match(/\w+\([^)]*\)/g) || [];
    var fns_b = to.match(/\w+\([^)]*\)/g) || [];
    if (fns_a.length !== fns_b.length) return t < 0.5 ? from : to;
    var out = [];
    for (var i = 0; i < fns_a.length; i++) {
        var fa = _wa_parse_tfn(fns_a[i]), fb = _wa_parse_tfn(fns_b[i]);
        if (!fa || !fb || fa.name !== fb.name) return t < 0.5 ? from : to;
        var args = [];
        for (var j = 0; j < fa.args.length; j++) args.push(_wa_lerp_scalar(fa.args[j], fb.args[j], t));
        out.push(fa.name + '(' + args.join(', ') + ')');
    }
    return out.join(' ');
}

// Interpolate a single CSS property value between two string values.
function _wa_interp_prop(prop, from, to, t) {
    if (from === to) return from;
    if (_wa_color_props[prop]) return _wa_lerp_color(from, to, t);
    if (prop === 'opacity') {
        var fa2 = parseFloat(from), fb2 = parseFloat(to);
        if (!isNaN(fa2) && !isNaN(fb2)) return String(+(fa2+(fb2-fa2)*t).toFixed(6));
    }
    if (prop === 'transform') return _wa_lerp_transform(from, to, t);
    return _wa_lerp_scalar(from, to, t);
}

// Compute the per-property interpolated styles for a KeyframeEffect at progress p.
function _wa_compute_at_p(effect, p) {
    var kfs = effect._keyframes;
    if (!kfs || !kfs.length) return {};
    // Find surrounding keyframe pair.
    var from = kfs[0], to = kfs[kfs.length - 1];
    for (var i = 0; i < kfs.length - 1; i++) {
        if (kfs[i].offset <= p && kfs[i+1].offset >= p) { from = kfs[i]; to = kfs[i+1]; break; }
    }
    var span = to.offset - from.offset;
    var lt = span < 1e-7 ? 1 : Math.max(0, Math.min(1, (p - from.offset) / span));
    lt = _wa_ease(lt, from.easing || 'linear');
    var result = {};
    for (var fp in from) {
        if (fp === 'offset' || fp === 'easing' || fp === 'composite') continue;
        result[fp] = (fp in to) ? _wa_interp_prop(fp, from[fp], to[fp], lt) : from[fp];
    }
    for (var tp in to) {
        if (tp === 'offset' || tp === 'easing' || tp === 'composite') continue;
        if (!(tp in result)) result[tp] = to[tp];
    }
    return result;
}

// Compute the iteration progress [0,1] from animation timing and currentTime.
function _wa_iter_progress(timing, ct) {
    var dur = +timing.duration || 0;
    if (dur <= 0) return 1;
    var delay = +(timing.delay || 0);
    var elapsed = ct - delay;
    var fill = timing.fill || 'auto';
    if (elapsed < 0) {
        return (fill === 'backwards' || fill === 'both') ? 0 : -1;
    }
    var maxIter = (timing.iterations === Infinity || timing.iterations == null) ? Infinity : +(timing.iterations) || 1;
    var totalDur = maxIter === Infinity ? Infinity : dur * maxIter;
    if (totalDur !== Infinity && elapsed >= totalDur) {
        return (fill === 'forwards' || fill === 'both') ? 1 : -2;
    }
    var iterFloor = Math.floor(elapsed / dur);
    var iterProg = (elapsed % dur) / dur;
    var dir = timing.direction || 'normal';
    var isOdd = iterFloor % 2 === 1;
    var directed = iterProg;
    if      (dir === 'reverse')           directed = 1 - iterProg;
    else if (dir === 'alternate')         directed = isOdd ? 1 - iterProg : iterProg;
    else if (dir === 'alternate-reverse') directed = isOdd ? iterProg : 1 - iterProg;
    return _wa_ease(Math.max(0, Math.min(1, directed)), timing.easing || 'linear');
}

// KeyframeEffect constructor (Web Animations §5.1).
function KeyframeEffect(target, keyframes, options) {
    this.target = target || null;
    this._keyframes = _wa_normalize_keyframes(keyframes);
    var opts = (typeof options === 'number') ? { duration: options } : (options || {});
    this._timing = {
        duration:       opts.duration     != null  ? +opts.duration       : 0,
        delay:          +(opts.delay      || 0),
        endDelay:       +(opts.endDelay   || 0),
        fill:           opts.fill         || 'auto',
        iterationStart: +(opts.iterationStart || 0),
        iterations:     opts.iterations   != null  ? opts.iterations      : 1,
        easing:         opts.easing       || 'linear',
        direction:      opts.direction    || 'normal',
    };
    this.composite          = opts.composite          || 'replace';
    this.iterationComposite = opts.iterationComposite || 'replace';
    this.pseudoElement      = opts.pseudoElement      || null;
}
KeyframeEffect.prototype.getTiming    = function() { return Object.assign({}, this._timing); };
KeyframeEffect.prototype.updateTiming = function(t) { Object.assign(this._timing, t); };
KeyframeEffect.prototype.getKeyframes = function() { return this._keyframes.slice(); };
KeyframeEffect.prototype.setKeyframes = function(kf) { this._keyframes = _wa_normalize_keyframes(kf); };

// Animation constructor (Web Animations §3.4).
// `Animation : EventTarget` (§5.3) — the three playback events (finish, cancel,
// remove) must reach `addEventListener` and not only the `on<type>` property,
// which is what BUG-808 was: a bare `addEventListener` call threw TypeError on
// the first line of every WPT setup that uses `EventWatcher`.
var _wa_anim_seq = 1;
function Animation(effect, timeline) {
    EventTarget.call(this);
    this._wid         = _wa_anim_seq++;
    this.id           = '';
    this.effect       = effect   || null;
    this.timeline     = timeline || _wa_doc_timeline;
    this._startTime   = null;
    this._holdTime    = null;
    this._pbRate      = 1;
    this._state       = 'idle';   // idle | running | paused | finished
    this._prevStyles  = {};
    this.onfinish     = null;
    this.oncancel     = null;
    this.onremove     = null;
    var self = this;
    this.ready    = Promise.resolve(self);
    this.finished = new Promise(function(res) { self._finishRes = res; });
    this._rafId   = null;
}
// Must precede every `Object.defineProperty(Animation.prototype, …)` below —
// replacing the prototype object afterwards would drop the accessors with it.
Animation.prototype = Object.create(EventTarget.prototype);
// Non-enumerable, like `constructor` on every real interface prototype:
// `web-animations/interfaces/Animation/style-change-events.html` builds one
// subtest per `Object.keys(Animation.prototype)` entry, so a plain assignment
// here invents a subtest named after an internal.
Object.defineProperty(Animation.prototype, 'constructor',
    { value: Animation, writable: true, configurable: true });

// Fire one playback event (Web Animations §4.4.3) at this animation.
// `EventTarget.prototype.dispatchEvent` runs the registered listeners and then
// the `on<type>` property, so both spellings work from one call site.
//
// The spec says «queue a task to fire an animation playback event», and the
// queueing is not a detail: WPT's own `EventWatcher` is armed AFTER the call
// that triggers the event (`new EventWatcher(t, anim, 'finish'); anim.finish();
// await watcher.wait_for('finish')`), so a synchronous dispatch arrives with no
// wait registered and the watcher fails it as «Not expecting event». The event
// object is still built now, so it carries the times as of the queueing.
// Written straight into `_lumen_timers` with `nesting: 0` rather than through
// setTimeout, for the same reason as `_ro_schedule_initial`: the §8.6 4 ms
// clamp must not apply.
// Non-enumerable — this is an internal, not an IDL member.
Object.defineProperty(Animation.prototype, '_fire', {
    value: function(type, currentTime) {
        var self = this;
        var ev = new AnimationPlaybackEvent(type, {
            currentTime:  (currentTime === undefined) ? this.currentTime : currentTime,
            timelineTime: this.timeline ? this.timeline.currentTime : null,
        });
        var deadline = _lumen_now_ms();
        _lumen_timers.push({
            id: _lumen_timer_seq++,
            fn: function() { self.dispatchEvent(ev); },
            deadline: deadline, interval: null, nesting: 0,
        });
        _lumen_request_wakeup(deadline);
    },
    writable: true, configurable: true,
});

Object.defineProperty(Animation.prototype, 'currentTime', {
    get: function() {
        if (this._holdTime !== null) return this._holdTime;
        if (this._startTime === null) return null;
        return (_wa_current_time - this._startTime) * this._pbRate;
    },
    set: function(v) {
        if (v == null) { this._holdTime = null; return; }
        this._holdTime = +v;
        if (this._state !== 'paused' && this._startTime !== null) {
            this._startTime = _wa_current_time - this._holdTime / this._pbRate;
            this._holdTime = null;
        }
    },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'startTime', {
    get: function() { return this._startTime; },
    set: function(v) {
        this._startTime = (v == null) ? null : +v;
        this._holdTime  = null;
        if (this._startTime !== null && this._state === 'idle') this._state = 'running';
    },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'playbackRate', {
    get: function() { return this._pbRate; },
    set: function(v) { this._pbRate = +v || 1; },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'playState', {
    get: function() { return this._state; },
    configurable: true,
});
Object.defineProperty(Animation.prototype, 'pending', {
    get: function() { return false; },
    configurable: true,
});

Animation.prototype.play = function() {
    var hold = this._holdTime !== null ? this._holdTime : (this._state === 'idle' ? 0 : null);
    if (hold !== null) {
        this._startTime = _wa_current_time - hold / this._pbRate;
        this._holdTime  = null;
    } else if (this._startTime === null) {
        this._startTime = _wa_current_time;
    }
    this._state = 'running';
    this._scheduleRaf();
    var idx = _wa_animations.indexOf(this);
    if (idx < 0) _wa_animations.push(this);
};

Animation.prototype.pause = function() {
    var ct = this.currentTime;
    this._holdTime  = ct !== null ? ct : 0;
    this._startTime = null;
    this._state     = 'paused';
    this._cancelRaf();
};

Animation.prototype.cancel = function() {
    this._clearStyles();
    this._state     = 'idle';
    this._startTime = null;
    this._holdTime  = null;
    this._cancelRaf();
    var idx = _wa_animations.indexOf(this);
    if (idx >= 0) _wa_animations.splice(idx, 1);
    // §4.4.1: the cancel event carries a null current time by definition.
    this._fire('cancel', null);
};

Animation.prototype.finish = function() {
    var eff = this.effect;
    if (eff) {
        var t = eff._timing;
        var maxI = (t.iterations === Infinity || t.iterations == null) ? Infinity : +t.iterations || 1;
        this._holdTime = maxI === Infinity ? 0 : +t.duration * maxI;
    }
    this._state = 'finished';
    this._applyAtP(1);
    this._cancelRaf();
    this._onFinish();
};

Animation.prototype.reverse = function() {
    this._pbRate = -this._pbRate;
    this.play();
};

Animation.prototype.updatePlaybackRate = function(rate) {
    this._pbRate = +rate || 1;
};

Animation.prototype._scheduleRaf = function() {
    if (this._rafId !== null) return;
    var self = this;
    this._rafId = requestAnimationFrame(function(ts) {
        self._rafId = null;
        self._tick(ts);
    });
};

Animation.prototype._cancelRaf = function() {
    if (this._rafId !== null) {
        cancelAnimationFrame(this._rafId);
        this._rafId = null;
    }
};

Animation.prototype._tick = function(now) {
    if (this._state !== 'running') return;
    var eff = this.effect;
    if (!eff) return;
    var ct = this.currentTime;
    if (ct === null) return;
    var p = _wa_iter_progress(eff._timing, ct);
    if (p === -2) {
        // Past end — finished
        this._state = 'finished';
        this._applyAtP(1);
        var idx = _wa_animations.indexOf(this);
        if (idx >= 0) _wa_animations.splice(idx, 1);
        this._onFinish();
        return;
    }
    if (p === -1) {
        // Before delay start — apply 'from' frame if fill=backwards|both
        var fillMode = (eff._timing && eff._timing.fill) || 'auto';
        if (fillMode === 'backwards' || fillMode === 'both') this._applyAtP(0);
    } else {
        this._applyAtP(p);
    }
    this._scheduleRaf();
};

Animation.prototype._applyAtP = function(p) {
    var eff = this.effect;
    if (!eff || !eff.target) return;
    var styles = _wa_compute_at_p(eff, p);
    for (var prop in styles) {
        try { eff.target.style[prop] = styles[prop]; } catch(e) {}
    }
    this._prevStyles = styles;
};

Animation.prototype._clearStyles = function() {
    var eff = this.effect;
    if (!eff || !eff.target) return;
    for (var prop in this._prevStyles) {
        try { eff.target.style[prop] = ''; } catch(e) {}
    }
    this._prevStyles = {};
};

Animation.prototype._onFinish = function() {
    this._fire('finish');
    if (typeof this._finishRes === 'function') { try { this._finishRes(this); } catch(e) {} this._finishRes = null; }
};

// §4.4.2 `remove` — dispatched when an animation is automatically replaced.
// The engine has no replacement machinery yet (BUG-704), so nothing calls this
// today; it exists so the event has one definition when replacement lands, and
// so `addEventListener('remove', …)` is already wired to it. Non-enumerable
// for the same reason as `_fire`.
Object.defineProperty(Animation.prototype, '_onRemove', {
    value: function() { this._fire('remove'); },
    writable: true, configurable: true,
});

// element.animate() factory shortcut (Web Animations §3.3).
function _wa_element_animate(target, keyframes, options) {
    var eff  = new KeyframeEffect(target, keyframes, options);
    var anim = new Animation(eff, _wa_doc_timeline);
    anim.play();
    return anim;
}

// element.getAnimations() — all non-idle animations targeting this element.
function _wa_get_animations_for(target) {
    return _wa_animations.filter(function(a) {
        return a._state !== 'idle' && a.effect && a.effect.target === target;
    });
}

// document.getAnimations() — all non-idle animations on this document.
function _wa_doc_get_animations() {
    return _wa_animations.filter(function(a) { return a._state !== 'idle'; });
}

// ── Web Locks API (W3C Web Locks API §5) ──────────────────────────────────────
// navigator.locks.request(name[, options], callback) → Promise
// navigator.locks.query() → Promise<{held, pending}>
//
// Single-context implementation: locks are scoped to one JS context (page).
// Cross-context coordination (cross-tab mutex) is Phase 3 / multi-process.
//
// Lock modes:
//   'exclusive' (default): one holder max; blocked by any existing lock.
//   'shared': concurrent readers allowed; blocked only by exclusive holders.
//
// Options (all optional):
//   mode       'exclusive' | 'shared'  (default 'exclusive')
//   signal     AbortSignal             (cancel queued request on abort)
//   ifAvailable boolean                (callback(null) if not immediately free)
//   steal      boolean                 (evict current holders; grant immediately)
(function() {
  var _locks = {};  // name → { excl, shared, queue[] }

  function _st(name) {
    if (!_locks[name]) _locks[name] = { excl: 0, shared: 0, queue: [] };
    return _locks[name];
  }

  function _canAcq(st, mode) {
    return mode === 'exclusive' ? st.excl === 0 && st.shared === 0 : st.excl === 0;
  }

  function _acq(st, mode) {
    if (mode === 'exclusive') st.excl++; else st.shared++;
  }

  function _rel(st, mode) {
    if (mode === 'exclusive') { if (st.excl   > 0) st.excl--;   }
    else                      { if (st.shared > 0) st.shared--; }
    _drain(st);
  }

  function _drain(st) {
    var i = 0;
    while (i < st.queue.length) {
      var req = st.queue[i];
      if (!_canAcq(st, req.mode)) break;
      _acq(st, req.mode);
      st.queue.splice(i, 1);
      req.grant();
      if (req.mode === 'exclusive') break; // exclusive acquired — stop draining
      // shared acquired — continue to try more queued shared requests
    }
  }

  function _run(cb, lock, resolve, reject, st, mode) {
    var res;
    try { res = cb(lock); } catch (e) { _rel(st, mode); reject(e); return; }
    Promise.resolve(res).then(
      function(v) { _rel(st, mode); resolve(v); },
      function(e) { _rel(st, mode); reject(e); }
    );
  }

  function Lock(name, mode) {
    Object.defineProperty(this, 'name', { value: name, enumerable: true });
    Object.defineProperty(this, 'mode', { value: mode, enumerable: true });
  }

  function LockManager() {}

  LockManager.prototype.request = function(name, a, b) {
    var opts = {}, cb;
    if (typeof a === 'function') { cb = a; }
    else { opts = a && typeof a === 'object' ? a : {}; cb = b; }

    if (typeof cb !== 'function')
      return Promise.reject(new TypeError('LockManager.request: callback required'));
    if (name == null)
      return Promise.reject(new TypeError('LockManager.request: name required'));

    name = String(name);
    var mode = opts.mode != null ? String(opts.mode) : 'exclusive';
    if (mode !== 'exclusive' && mode !== 'shared')
      return Promise.reject(
        new TypeError('LockManager.request: mode must be exclusive or shared'));

    var sig    = opts.signal     || null;
    var ifAvl  = !!opts.ifAvailable;
    var steal  = !!opts.steal;
    var st     = _st(name);

    if (steal) {
      // Evict all current holders and remove exclusive pending requests.
      st.excl = 0; st.shared = 0;
      for (var qi = st.queue.length - 1; qi >= 0; qi--) {
        if (st.queue[qi].mode === 'exclusive') {
          st.queue[qi].abort(new DOMException('Lock stolen', 'AbortError'));
          st.queue.splice(qi, 1);
        }
      }
    }

    return new Promise(function(resolve, reject) {
      if (sig && sig.aborted) {
        reject(sig.reason instanceof Error ? sig.reason
          : new DOMException('The operation was aborted.', 'AbortError'));
        return;
      }
      if (_canAcq(st, mode)) {
        _acq(st, mode);
        _run(cb, new Lock(name, mode), resolve, reject, st, mode);
        return;
      }
      if (ifAvl) {
        var r2;
        try { r2 = cb(null); } catch (e2) { reject(e2); return; }
        Promise.resolve(r2).then(resolve, reject);
        return;
      }
      // Queue the request.
      var granted = false, abortH = null;
      function onGrant() {
        if (granted) return; granted = true;
        if (sig && abortH) sig.removeEventListener('abort', abortH);
        _run(cb, new Lock(name, mode), resolve, reject, st, mode);
      }
      function onAbort() {
        if (granted) return;
        for (var j = 0; j < st.queue.length; j++) {
          if (st.queue[j].grant === onGrant) { st.queue.splice(j, 1); break; }
        }
        var reason = (sig && sig.reason instanceof Error)
          ? sig.reason : new DOMException('The operation was aborted.', 'AbortError');
        reject(reason);
      }
      if (sig) { abortH = onAbort; sig.addEventListener('abort', abortH); }
      st.queue.push({ mode: mode, grant: onGrant, abort: onAbort });
    });
  };

  LockManager.prototype.query = function() {
    var held = [], pending = [];
    for (var n in _locks) {
      var s = _locks[n];
      for (var i = 0; i < s.excl;   i++) held.push({ name: n, mode: 'exclusive', clientId: '' });
      for (var j = 0; j < s.shared; j++) held.push({ name: n, mode: 'shared',    clientId: '' });
      for (var k = 0; k < s.queue.length; k++)
        pending.push({ name: n, mode: s.queue[k].mode, clientId: '' });
    }
    return Promise.resolve({ held: held, pending: pending });
  };

  var _lockMgr = new LockManager();
  Object.defineProperty(navigator, 'locks', {
    value: _lockMgr, configurable: true, writable: false, enumerable: true,
  });
  window.LockManager = LockManager;
  window.Lock = Lock;
})();

// ── Screen Wake Lock API (W3C Screen Wake Lock §6.5) ──────────────────────────
// navigator.wakeLock.request('screen') → Promise<WakeLockSentinel>
// Phase 1 stub: always resolves (no OS integration yet; release is a no-op).
(function() {
  function WakeLockSentinel(type) {
    Object.defineProperty(this, 'type', { value: type, enumerable: true });
    this.released  = false;
    this._listeners = [];
  }
  WakeLockSentinel.prototype.release = function() {
    if (this.released) return Promise.resolve();
    this.released = true;
    var ev = { type: 'release', target: this };
    if (typeof this._onrelease === 'function') try { this._onrelease(ev); } catch(e) { _lumen_report_exception(e); }
    for (var i = 0; i < this._listeners.length; i++) try { this._listeners[i](ev); } catch(e) { _lumen_report_exception(e); }
    return Promise.resolve();
  };
  Object.defineProperty(WakeLockSentinel.prototype, 'onrelease', {
    get: function() { return this._onrelease || null; },
    set: function(fn) { this._onrelease = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });
  WakeLockSentinel.prototype.addEventListener = function(t, fn) {
    if (t === 'release' && typeof fn === 'function') this._listeners.push(fn);
  };
  WakeLockSentinel.prototype.removeEventListener = function(t, fn) {
    var i = this._listeners.indexOf(fn); if (i >= 0) this._listeners.splice(i, 1);
  };

  navigator.wakeLock = {
    request: function(type) {
      if (type !== 'screen')
        return Promise.reject(
          new DOMException('Unsupported wake lock type: ' + String(type), 'NotSupportedError'));
      return Promise.resolve(new WakeLockSentinel(String(type)));
    },
  };
  window.WakeLockSentinel = WakeLockSentinel;
})();

// ── Network Information API (W3C Network Information §7) ──────────────────────
// navigator.connection — effective type, downlink, rtt, saveData.
// Phase 1 stub: reports '4g'/10 Mbps/100 ms (reasonable desktop default).
(function() {
  function NetworkInformation() {
    this.effectiveType = '4g';
    this.downlink      = 10;
    this.rtt           = 100;
    this.saveData      = false;
    this.type          = 'wifi';
    this._onchange     = null;
  }
  Object.defineProperty(NetworkInformation.prototype, 'onchange', {
    get: function() { return this._onchange; },
    set: function(fn) { this._onchange = typeof fn === 'function' ? fn : null; },
    configurable: true,
  });
  NetworkInformation.prototype.addEventListener    = function() {};
  NetworkInformation.prototype.removeEventListener = function() {};

  navigator.connection = new NetworkInformation();
  window.NetworkInformation = NetworkInformation;
})();

// ── navigator.userActivation (HTML LS §6.4) ───────────────────────────────────
// Single-user interactive desktop app: always reports the user has activated.
Object.defineProperty(navigator, 'userActivation', {
  value: Object.freeze({ isActive: true, hasBeenActive: true }),
  configurable: true, writable: false, enumerable: true,
});

// ── Web Share API (W3C Web Share §4) ──────────────────────────────────────────
// Phase 1 stub: always rejects (no OS share-sheet integration yet).
navigator.share = function(_data) {
  return Promise.reject(
    new DOMException('navigator.share is not supported in Lumen Phase 1.', 'NotSupportedError'));
};
navigator.canShare = function() { return false; };

// ── window.reportError() (HTML LS §8.1.3.6) ───────────────────────────────────
// Fires an ErrorEvent on window for the given error (uncaught-error pipeline).
function reportError(err) {
  var msg = err instanceof Error ? err.message : String(err);
  var ev = new ErrorEvent('error', { error: err, message: msg, bubbles: true, cancelable: true });
  window.dispatchEvent(ev);
}
window.reportError = reportError;

// ── DOM GC collect (idle shell tick) ─────────────────────────────────────────
// Called by the shell's GcTick every 30 s with an array of node IDs that
// have been detached from the document and have zero live JS references.
// Purges JS-side per-node caches so dead nodes don't retain memory through maps:
//   - _lumen_listeners        keyed by 'nid:eventtype'
//   - _lumen_on_handlers      keyed by 'nid:type' (BUG-360 on<type> IDL attributes)
//   - _input_values           keyed by nid
//   - _lumen_element_wrappers keyed by nid (BUG-291 identity cache)
// The arena itself is append-only in Phase 1; physical compaction is Phase 3.
function _lumen_gc_collect(nids) {
    for (var i = 0; i < nids.length; i++) {
        var nid = nids[i];
        var prefix = String(nid) + ':';
        var plen   = prefix.length;
        for (var key in _lumen_listeners) {
            if (key.length > plen && key.substring(0, plen) === prefix) {
                delete _lumen_listeners[key];
            }
        }
        for (var okey in _lumen_on_handlers) {
            if (okey.length > plen && okey.substring(0, plen) === prefix) {
                delete _lumen_on_handlers[okey];
            }
        }
        delete _input_values[nid];
        // BUG-441: the control's runtime value lives document-side; a dead node
        // must not keep its slot in that map either.
        _lumen_clear_dirty_value(nid);
        // BUG-444: same for the control's runtime checkedness.
        _lumen_clear_dirty_checked(nid);
        delete _canvas2d_ctxs[nid];
        delete _lumen_element_wrappers[nid];
        // BUG-383 per-nid form state.
        delete _lumen_option_selected[nid];
        delete _lumen_selection_state[nid];
        delete _lumen_indeterminate[nid];
        delete _lumen_click_in_progress[nid];
    }
}

// B-7: CSS Resize property Phase 1 — apply element width/height changes from grip drag.
// Called during CursorMoved when resize_active is set.
// start_x/y are saved at MouseInput Pressed; delta is computed from current cursor position.
// The binding updates element's inline style: width = computed_width + delta_x; height = computed_height + delta_y.
function _lumen_apply_resize(nid, delta_x, delta_y) {
    var elem = _lumen_make_element(nid);
    if (!elem) return;

    var style = elem.style;
    if (!style) return;

    // Get current computed dimensions (bounding rect: [x, y, w, h])
    var rect = _lumen_get_bounding_rect(nid);
    if (!rect) return;

    var curr_width = rect[2];
    var curr_height = rect[3];

    // Apply delta to compute new width/height
    var new_width = Math.max(0, curr_width + delta_x);
    var new_height = Math.max(0, curr_height + delta_y);

    // Update inline style (will trigger relayout + repaint)
    style.width = new_width + 'px';
    style.height = new_height + 'px';
}

// D-6: Extension system stub — chrome.runtime API Phase 0.
// Provides enough surface so existing extension content-scripts don't throw on import.
// Phase 0: sendMessage is fire-and-forget (message goes to native no-op binding).
// Phase 1: shell wires up a real message bus between content scripts and extension background.
// Guard: only install when _LUMEN_EXTENSION_ACTIVE is set (avoids CDP automation detection markers).
(function() {
    if (typeof globalThis === 'undefined' || !globalThis._LUMEN_EXTENSION_ACTIVE) { return; }
    var _rt = {
        id: 'lumen-extension',
        sendMessage: function(msg, callback) {
            _lumen_chrome_runtime_send_message(JSON.stringify(msg));
            if (typeof callback === 'function') { callback(undefined); }
        },
        onMessage: {
            _listeners: [],
            addListener: function(fn) { this._listeners.push(fn); },
            removeListener: function(fn) {
                this._listeners = this._listeners.filter(function(l) { return l !== fn; });
            },
            hasListener: function(fn) { return this._listeners.indexOf(fn) !== -1; }
        },
        getURL: function(path) { return 'chrome-extension://lumen-extension/' + path; },
        getManifest: function() { return { name: '', version: '0', manifest_version: 3 }; }
    };
    if (typeof globalThis !== 'undefined') {
        globalThis.chrome = { runtime: _rt };
        globalThis.browser = { runtime: _rt };
    }
    if (typeof window !== 'undefined') {
        window.chrome = { runtime: _rt };
        window.browser = { runtime: _rt };
    }
})();

// ── scroll events helpers ──────────────────────────────────────────────────────
// Called from Rust (QuickJsRuntime::fire_element_scroll / fire_window_scroll)
// after scroll position changes.  Per WHATWG HTML §8.1.6.2 scroll events are
// non-bubbling (bubbles:false) and non-cancelable.
function _lumen_fire_scroll_on_element(nid) {
    var el = _lumen_make_element(nid);
    if (!el) return;
    var ev = new Event('scroll', { bubbles: false, cancelable: false });
    el.dispatchEvent(ev);
}
function _lumen_fire_window_scroll_event() {
    var ev = new Event('scroll', { bubbles: false, cancelable: false });
    if (typeof window !== 'undefined') { window.dispatchEvent(ev); }
    if (typeof document !== 'undefined') { document.dispatchEvent(ev); }
}
// BUG-822: the `scrollend` half of the same pair (CSSOM-View §14 «scrollend»).
// The shell calls these once a scrolling sequence has *completed*, which for an
// instant scroll is the very rendering update that also fired `scroll` — the
// spec explicitly allows both in one frame — and for a smooth animation or
// touch momentum is the update on which it stopped driving the position.
// Like `scroll`, `scrollend` is non-bubbling and non-cancelable.
function _lumen_fire_scrollend_on_element(nid) {
    var el = _lumen_make_element(nid);
    if (!el) return;
    var ev = new Event('scrollend', { bubbles: false, cancelable: false });
    el.dispatchEvent(ev);
}
function _lumen_fire_window_scrollend_event() {
    var ev = new Event('scrollend', { bubbles: false, cancelable: false });
    if (typeof window !== 'undefined') { window.dispatchEvent(ev); }
    if (typeof document !== 'undefined') { document.dispatchEvent(ev); }
}

// FRAME-1: fired on a sub-document's window when its viewport (the host
// `<iframe>`'s content box) actually changes size (`frames.rs::sync_frame_viewports`).
// Per HTML LS §7.4.4 the resize event targets `window` only — unlike `scroll`,
// it has no legacy `document`-target form.
function _lumen_fire_window_resize_event() {
    var ev = new Event('resize', { bubbles: false, cancelable: false });
    if (typeof window !== 'undefined') { window.dispatchEvent(ev); }
}

// ── WindowOrWorkerGlobalScope: window IS the real global object (HTML LS) ──
// In a real browser self === window === globalThis === the JS engine's own
// global object — so ANY property a script assigns via `window.foo = ...` /
// `self.foo = ...` is automatically reachable afterward as a bare, unqualified
// `foo` identifier. Up to this point `window` has been built as a plain object
// literal (see `var window = {...}` above), merely cross-referenced with
// `globalThis` for a hardcoded list of names (`self`, `addEventListener`, …
// this was BUG-233's fix). That only covers names known in advance — dynamic
// assignments (e.g. `testharness.js`'s `expose(fn, name)`, which does
// `window[name] = fn`, or any real-world `window.foo = ...; foo()` pattern)
// stayed invisible as bare identifiers, because `window` and the true global
// object were two different objects (BUG-280).
//
// Fix: copy every property built onto `window` so far onto `globalThis` once,
// then repoint `window` to literally BE `globalThis`. From here on `window`
// and the engine's real global object are the SAME reference, so later
// `window.foo = ...` writes land directly on the global object and are
// bare-reachable — no finite alias list needed.
//
// Copy accessors (get/set) via `defineProperty` and plain values via assignment
// — two different copy strategies, both needed:
//  - `Object.assign` alone would invoke every getter once and copy the
//    resulting *value* (e.g. `scrollY`/`pageYOffset` call
//    `_lumen_get_page_scroll_y()` on every read), freezing it as a static
//    data property and silently breaking the live binding — so accessors need
//    `defineProperty` to carry the getter/setter itself across.
//  - `defineProperty` for EVERY property would instead break plain values
//    that already exist on `globalThis` as non-configurable (but writable)
//    data properties — e.g. quickjs-ng's built-in `addEventListener` —
//    because `defineProperty` fails when the target's existing descriptor
//    isn't configurable, even to set the same kind of value. Plain
//    assignment (`globalThis[k] = ...`) only invokes [[Set]], which is
//    exactly what non-configurable-but-writable globals still allow.
(function() {
    var descs = Object.getOwnPropertyDescriptors(window);
    for (var k in descs) {
        var d = descs[k];
        if (d.get || d.set) {
            Object.defineProperty(globalThis, k, d);
        } else {
            globalThis[k] = d.value;
        }
    }
})();
window = globalThis;
var self = window;
window.self          = window;
window.window        = window;   // window.window === window (HTML LS)
window.globalThis    = globalThis;
window.frames        = window;   // no real framesets; self-reference like browsers
window.top           = window;   // top-level browsing context is itself
window.parent        = window;   // no parent frame
window.length        = 0;        // number of child browsing contexts (frames)

// addEventListener/removeEventListener/dispatchEvent now resolve as bare
// identifiers because `window` (just reassigned above) IS the global object —
// this rebind is mostly for clarity, since the copy loop above already put the
// raw functions onto globalThis. Kept explicit and bound to `window` so
// `this` inside these methods is well-defined regardless of call style.
var addEventListener    = window.addEventListener.bind(window);
var removeEventListener = window.removeEventListener.bind(window);
var dispatchEvent       = window.dispatchEvent.bind(window);
