// ── TextEncoder / TextDecoder (WHATWG Encoding §8–9) ─────────────────────────
// encode() stays a pure-JS UTF-8 encoder (the encoder is always UTF-8 per
// spec). Decoding — label canonicalization, RangeError on unknown labels,
// real multi-encoding decode and fatal-mode error detection — is bridged to
// the native `_lumen_text_decode`/`_lumen_text_encoding_for_label` functions
// (crates/js/src/v8_runtime.rs), backed by `lumen_encoding` (BUG-357). That
// decoder is stateless (whole-buffer in, `String` out — no incremental
// decoder object), so streaming reassembly (holding back a byte sequence a
// chunk boundary split mid-character) and the only-strip-a-BOM-on-the-first-
// chunk-of-a-stream rule for `ignoreBOM` are handled here in JS.
//
// Supported encodings match `lumen_encoding::Encoding` — UTF-8/16/32,
// windows-1251, KOI8-R, IBM866 — the Cyrillic-web + Unicode set this browser
// actually implements (`docs/plan/tech-stack.md` deliberately rejects
// `encoding_rs`/hand-porting the full ~40-encoding WHATWG set in favor of
// this crate's own tables). A label for any other real-but-unimplemented
// encoding (Shift_JIS, GBK, windows-1252, …) is treated the same as an
// unknown label: `_lumen_text_encoding_for_label` returns undefined and the
// constructor throws `RangeError` — a deliberate scope decision, not a bug.

function TextEncoder() {}
Object.defineProperty(TextEncoder.prototype, 'encoding', {
    value: 'utf-8', enumerable: true, configurable: true
});
TextEncoder.prototype.encode = function(str) {
    var s = String(str === undefined ? '' : str);
    var bytes = [];
    for (var i = 0; i < s.length; i++) {
        var c = s.charCodeAt(i);
        if (c < 0x80) {
            bytes.push(c);
        } else if (c < 0x800) {
            bytes.push(0xC0 | (c >> 6));
            bytes.push(0x80 | (c & 0x3F));
        } else if (c >= 0xD800 && c <= 0xDBFF && i + 1 < s.length) {
            var lo = s.charCodeAt(i + 1);
            var cp = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
            bytes.push(0xF0 | (cp >> 18));
            bytes.push(0x80 | ((cp >> 12) & 0x3F));
            bytes.push(0x80 | ((cp >> 6) & 0x3F));
            bytes.push(0x80 | (cp & 0x3F));
            i++;
        } else {
            bytes.push(0xE0 | (c >> 12));
            bytes.push(0x80 | ((c >> 6) & 0x3F));
            bytes.push(0x80 | (c & 0x3F));
        }
    }
    return new Uint8Array(bytes);
};
// Encoding §6.2 encodeInto — same per-code-unit encoding as encode(), but
// writes directly into `dest` and stops once it runs out of room, reporting
// how many UTF-16 code units of `src` were consumed and bytes written. Never
// splits a surrogate pair or a multi-byte UTF-8 sequence across the boundary.
TextEncoder.prototype.encodeInto = function(src, dest) {
    var s = String(src === undefined ? '' : src);
    var read = 0, written = 0, i = 0;
    while (i < s.length) {
        var c = s.charCodeAt(i);
        var unitLen = 1, out;
        if (c < 0x80) {
            out = [c];
        } else if (c < 0x800) {
            out = [0xC0 | (c >> 6), 0x80 | (c & 0x3F)];
        } else if (c >= 0xD800 && c <= 0xDBFF && i + 1 < s.length) {
            var lo = s.charCodeAt(i + 1);
            var cp = 0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00);
            out = [0xF0 | (cp >> 18), 0x80 | ((cp >> 12) & 0x3F), 0x80 | ((cp >> 6) & 0x3F), 0x80 | (cp & 0x3F)];
            unitLen = 2;
        } else {
            out = [0xE0 | (c >> 12), 0x80 | ((c >> 6) & 0x3F), 0x80 | (c & 0x3F)];
        }
        if (written + out.length > dest.length) break;
        for (var k = 0; k < out.length; k++) dest[written + k] = out[k];
        written += out.length;
        read += unitLen;
        i += unitLen;
    }
    return { read: read, written: written };
};

// Returns how many trailing bytes of `bytes` belong to a code unit/sequence
// the buffer cuts off mid-way, for the multi-byte encoding families where a
// streaming chunk boundary can land inside a character. Those bytes must be
// held back and prepended to the next chunk instead of being decoded now.
// Single-byte encodings (windows-1251/koi8-r/ibm866) never have a pending
// remainder — every byte stands alone.
function _lumenTextPendingTailLen(canonical, bytes) {
    var n = bytes.length;
    if (canonical === 'utf-8') {
        var i = 0;
        while (i < n) {
            var b = bytes[i];
            var seqLen;
            if (b < 0x80) { seqLen = 1; }
            else if ((b & 0xE0) === 0xC0) { seqLen = 2; }
            else if ((b & 0xF0) === 0xE0) { seqLen = 3; }
            else if ((b & 0xF8) === 0xF0) { seqLen = 4; }
            else { i++; continue; } // stray/invalid byte — not a pending lead
            if (i + seqLen > n) { return n - i; }
            i += seqLen;
        }
        return 0;
    }
    if (canonical === 'utf-16le' || canonical === 'utf-16be') { return n % 2; }
    if (canonical === 'utf-32le' || canonical === 'utf-32be') { return n % 4; }
    return 0;
}

function TextDecoder(label, options) {
    var canonical = _lumen_text_encoding_for_label(label === undefined ? 'utf-8' : String(label));
    if (canonical === undefined) {
        throw new RangeError("Failed to construct 'TextDecoder': The encoding label provided ('" + label + "') is invalid.");
    }
    this._encoding = canonical;
    this._fatal = !!(options && options.fatal);
    this._ignoreBOM = !!(options && options.ignoreBOM);
    this._pending = null;   // bytes held back from a previous streaming chunk
    this._sawInput = false; // BOM stripping applies only to a stream's first chunk
}
Object.defineProperty(TextDecoder.prototype, 'encoding', {
    get: function() { return this._encoding; }, enumerable: true, configurable: true
});
Object.defineProperty(TextDecoder.prototype, 'fatal', {
    get: function() { return this._fatal; }, enumerable: true, configurable: true
});
Object.defineProperty(TextDecoder.prototype, 'ignoreBOM', {
    get: function() { return this._ignoreBOM; }, enumerable: true, configurable: true
});
// Encoding Standard §9.1 decode(). The native `_lumen_text_decode` call does
// the actual per-encoding decode and fatal-mode error detection (signalled
// by returning undefined); this wrapper reassembles streaming chunks, keeps
// an incomplete trailing sequence for the next call, and turns the native
// malformed-input signal into the spec-mandated TypeError.
TextDecoder.prototype.decode = function(buf, options) {
    var stream = !!(options && options.stream);
    var input;
    if (buf === undefined || buf === null) {
        input = new Uint8Array(0);
    } else {
        input = buf instanceof Uint8Array ? buf : new Uint8Array(buf instanceof ArrayBuffer ? buf : new ArrayBuffer(0));
    }
    var bytes;
    if (this._pending && this._pending.length > 0) {
        var combined = new Uint8Array(this._pending.length + input.length);
        combined.set(this._pending);
        combined.set(input, this._pending.length);
        bytes = combined;
    } else {
        bytes = input;
    }
    this._pending = null;

    var toDecode = bytes;
    if (stream) {
        var pendLen = _lumenTextPendingTailLen(this._encoding, bytes);
        if (pendLen > 0) {
            this._pending = bytes.slice(bytes.length - pendLen);
            toDecode = bytes.slice(0, bytes.length - pendLen);
        }
    }

    // A BOM is only meaningful at the start of a decode session — pass
    // ignoreBOM=true (suppress stripping) on every chunk after the first so a
    // BOM-like byte sequence arriving mid-stream is decoded as plain content.
    var ignoreBOMForThisCall = this._sawInput ? true : this._ignoreBOM;
    this._sawInput = true;

    var result = _lumen_text_decode(this._encoding, toDecode, ignoreBOMForThisCall, this._fatal);
    if (result === undefined) {
        throw new TypeError('Failed to decode: The encoded data was not valid ' + this._encoding + ' data.');
    }
    if (!stream) {
        // Encoding Standard: a non-streaming decode() always ends the session
        // — the next call, streaming or not, starts fresh.
        this._pending = null;
        this._sawInput = false;
    }
    return result;
};

