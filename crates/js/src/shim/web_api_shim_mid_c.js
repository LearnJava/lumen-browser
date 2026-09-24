
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
function atob(str) {
    var s = String(str).replace(/[ \t\r\n]+/g, '');
    var valid = true;
    for (var _i = 0; _i < s.length; _i++) {
        var _c = s.charCodeAt(_i);
        if (!((_c >= 65 && _c <= 90) || (_c >= 97 && _c <= 122) ||
              (_c >= 48 && _c <= 57) || _c === 43 || _c === 47 || _c === 61))
            { valid = false; break; }
    }
    if (s.length % 4 !== 0 || !valid)
        throw new DOMException('atob: invalid base64 string', 'InvalidCharacterError');
    var idx = {}, i; for (i = 0; i < _b64c.length; i++) idx[_b64c[i]] = i;
    var out = '';
    for (var j = 0; j < s.length; j += 4) {
        var n = (idx[s[j]] << 18) | (idx[s[j+1]] << 12) |
                ((s[j+2] === '=' ? 0 : idx[s[j+2]]) << 6) |
                (s[j+3] === '=' ? 0 : idx[s[j+3]]);
        out += String.fromCharCode(n >> 16);
        if (s[j+2] !== '=') out += String.fromCharCode((n >> 8) & 0xff);
        if (s[j+3] !== '=') out += String.fromCharCode(n & 0xff);
    }
    return out;
}

