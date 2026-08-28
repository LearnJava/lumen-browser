
// ── btoa / atob (HTML5 Living Std §2.4.7 + RFC 4648 §4) ─────────────────────
var _b64c = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
function btoa(str) {
    var s = String(str), out = '';
    for (var i = 0; i < s.length; i++) {
        if (s.charCodeAt(i) > 0xff) throw new TypeError('btoa: character out of Latin1 range');
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
        throw new TypeError('atob: invalid base64 string');
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

// ── Blob / File / FileReader (WHATWG File API) ────────────────────────────────
function _blob_concat_parts(parts) {
    if (!parts || !parts.length) return new Uint8Array(0);
    var arrays = [], total = 0, enc = new TextEncoder();
    for (var i = 0; i < parts.length; i++) {
        var p = parts[i], a;
        if (typeof p === 'string') {
            a = enc.encode(p);
        } else if (p && p._bytes instanceof Uint8Array) {
            // Blob or File
            a = p._bytes;
        } else if (p instanceof ArrayBuffer) {
            a = new Uint8Array(p);
        } else if (p && ArrayBuffer.isView(p)) {
            a = new Uint8Array(p.buffer, p.byteOffset, p.byteLength);
        } else {
            a = enc.encode(String(p));
        }
        arrays.push(a);
        total += a.length;
    }
    var out = new Uint8Array(total), off = 0;
    for (var j = 0; j < arrays.length; j++) { out.set(arrays[j], off); off += arrays[j].length; }
    return out;
}

// WHATWG File API §4 — Blob
function Blob(blobParts, options) {
    this._bytes = _blob_concat_parts(blobParts || []);
    this._type  = (options && typeof options.type === 'string')
        ? options.type.toLowerCase() : '';
}
Object.defineProperties(Blob.prototype, {
    size: { get: function() { return this._bytes.length; }, enumerable: true },
    type: { get: function() { return this._type; }, enumerable: true },
});
Blob.prototype.slice = function(start, end, contentType) {
    var len = this._bytes.length;
    var s = typeof start === 'number' ? (start < 0 ? Math.max(len+start,0) : Math.min(start,len)) : 0;
    var e = typeof end   === 'number' ? (end   < 0 ? Math.max(len+end,  0) : Math.min(end,  len)) : len;
    if (e < s) e = s;
    return new Blob([this._bytes.slice(s, e)],
        { type: typeof contentType === 'string' ? contentType : this._type });
};
Blob.prototype.text = function() {
    return Promise.resolve(new TextDecoder().decode(this._bytes));
};
Blob.prototype.arrayBuffer = function() {
    return Promise.resolve(this._bytes.buffer.slice(0));
};
Blob.prototype.stream = function() {
    var bytes = this._bytes;
    return new ReadableStream({
        start: function(c) { c.enqueue(bytes); c.close(); }
    });
};

// WHATWG File API §7 — File extends Blob
function File(fileParts, fileName, options) {
    if (fileName === undefined) throw new TypeError('File requires a fileName argument');
    Blob.call(this, fileParts, options);
    this._name = String(fileName);
    this._lastModified = (options && typeof options.lastModified === 'number')
        ? options.lastModified : Date.now();
}
File.prototype = Object.create(Blob.prototype);
File.prototype.constructor = File;
Object.defineProperties(File.prototype, {
    name:             { get: function() { return this._name; }, enumerable: true },
    lastModified:     { get: function() { return this._lastModified; }, enumerable: true },
    lastModifiedDate: { get: function() { return new Date(this._lastModified); }, enumerable: true },
});

// WHATWG File API §10 — FileReader
function FileReader() {
    this.readyState = FileReader.EMPTY;
    this.result     = null;
    this.error      = null;
    this.onloadstart = null;
    this.onprogress  = null;
    this.onload      = null;
    this.onabort     = null;
    this.onerror     = null;
    this.onloadend   = null;
    this._aborted    = false;
}
FileReader.EMPTY   = 0;
FileReader.LOADING = 1;
FileReader.DONE    = 2;
FileReader.prototype._dispatch = function(name, extra) {
    var ev = Object.assign({ type: name, target: this }, extra || {});
    var h = this['on' + name];
    if (typeof h === 'function') h.call(this, ev);
};
FileReader.prototype._doRead = function(blob, transform) {
    var self = this;
    self.readyState = FileReader.LOADING;
    self.result = null; self.error = null; self._aborted = false;
    self._dispatch('loadstart');
    queueMicrotask(function() {
        if (self._aborted) {
            self.readyState = FileReader.DONE; self.result = null;
            self._dispatch('abort'); self._dispatch('loadend'); return;
        }
        try {
            self.result = transform(blob);
            self.readyState = FileReader.DONE;
            self._dispatch('load');
        } catch(err) {
            self.readyState = FileReader.DONE;
            self.error = { name: 'NotReadableError', message: String(err) };
            self._dispatch('error');
        }
        self._dispatch('loadend');
    });
};
FileReader.prototype.readAsText = function(blob, encoding) {
    var dec = new TextDecoder(encoding || 'utf-8');
    this._doRead(blob, function(b) { return dec.decode(b._bytes); });
};
FileReader.prototype.readAsArrayBuffer = function(blob) {
    this._doRead(blob, function(b) { return b._bytes.buffer.slice(0); });
};
FileReader.prototype.readAsBinaryString = function(blob) {
    this._doRead(blob, function(b) {
        var s = '';
        for (var i = 0; i < b._bytes.length; i++) s += String.fromCharCode(b._bytes[i]);
        return s;
    });
};
FileReader.prototype.readAsDataURL = function(blob) {
    this._doRead(blob, function(b) {
        var s = '';
        for (var i = 0; i < b._bytes.length; i++) s += String.fromCharCode(b._bytes[i]);
        return 'data:' + (b._type || 'application/octet-stream') + ';base64,' + btoa(s);
    });
};
FileReader.prototype.abort = function() {
    if (this.readyState === FileReader.LOADING) this._aborted = true;
};

// WHATWG File API §24.9 — URL.createObjectURL / revokeObjectURL
var _object_url_store = Object.create(null);
var _object_url_seq   = 0;
URL.createObjectURL = function(blob) {
    var key = 'blob:lumen/' + (++_object_url_seq);
    _object_url_store[key] = blob;
    return key;
};
URL.revokeObjectURL = function(url) { delete _object_url_store[String(url)]; };
