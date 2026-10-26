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

    // `globalThis`, not `window`: the same object on the page (`window` is the real global,
    // BUG-280), and a worker scope has no `window` (BUG-1688).
    var g = globalThis;
    g.crypto = { getRandomValues: getRandomValues, randomUUID: randomUUID };
    g.Crypto = function Crypto() {};
    // BUG-765: `SubtleCrypto` and `CryptoKey` are both `[SecureContext]`
    // (WebCryptoAPI) — `crypto.getRandomValues`/`.randomUUID` are not, so
    // only `.subtle` and the `CryptoKey` constructor are gated, not the
    // whole `Crypto` interface. Absent entirely on an insecure origin, not
    // merely inert (see `_lumen_secure_context`'s doc comment, `web_api_shim_mid_b.js`).
    function exposeSecure() {
        g.crypto.subtle = subtle;
        g.CryptoKey = CryptoKey;
    }
    if (typeof _lumen_secure_context !== 'undefined') {
        if (_lumen_secure_context !== false) exposeSecure();
    } else {
        // A worker scope (BUG-1688): `isSecureContext` is defined by the flavour's own shim
        // (it needs the worker URL, set after this shim runs), which then calls this hook once
        // and removes it.
        g._lumen_crypto_expose_secure = exposeSecure;
    }
})();
