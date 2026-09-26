
// ── btoa / atob (HTML5 Living Std §2.4.7 + RFC 4648 §4) ─────────────────────
var _b64c = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function btoa(str) {
    var s = String(str), out = '';
    for (var i = 0; i < s.length; i++) {
        if (s.charCodeAt(i) > 0xff) throw new DOMException('btoa: character out of Latin1 range', 'InvalidCharacterError');
    }
    for (var j = 0; j < s.length; j += 3) {
        var b0 = s.charCodeAt(j), b1 = s.charCodeAt(j+1) || 0, b2 = s.charCodeAt(j+2) || 0;
        out += _b64c[b0 >> 2];
        out += _b64c[((b0 & 3) << 4) | (b1 >> 4)];
        out += j+1 < s.length ? _b64c[((b1 & 0xf) << 2) | (b2 >> 6)] : '=';
        out += j+2 < s.length ? _b64c[b2 & 0x3f]                      : '=';
    }
    return out;
}
// Infra "forgiving-base64 decode" (BUG-1133): padding is optional, `=` only
// as 1-2 trailing chars of a length-multiple-of-4 input. Same algorithm as the
// worker native `forgiving_b64_decode` in `crates/js/src/worker.rs`.
function atob(str) {
    var s = String(str).replace(/[\t\n\f\r ]+/g, '');
    if (s.length % 4 === 0) s = s.replace(/==?$/, '');
    if (s.length % 4 === 1 || /[^A-Za-z0-9+\/]/.test(s))
        throw new DOMException('atob: invalid base64 string', 'InvalidCharacterError');
    var out = '', buf = 0, bits = 0;
    for (var j = 0; j < s.length; j++) {
        buf = ((buf << 6) | _b64c.indexOf(s[j])) & 0xffffff;
        bits += 6;
        if (bits >= 8) { bits -= 8; out += String.fromCharCode((buf >> bits) & 0xff); }
    }
    return out;
}

