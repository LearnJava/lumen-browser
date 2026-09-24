// ── FormData (XHR Spec §4 / Fetch Spec) ────────────────────────────────────
// Stores an ordered list of (name, value) pairs. Values are always strings
// (File/Blob support is Phase 2+). Serializes to application/x-www-form-urlencoded.

function FormData(formEl) {
    this._entries = [];
    if (formEl && typeof formEl === 'object' && formEl.tagName === 'FORM') {
        var inputs = formEl.querySelectorAll('input,select,textarea');
        for (var i = 0; i < inputs.length; i++) {
            var el = inputs[i];
            var name = el.getAttribute('name');
            if (!name) { continue; }
            var type = (el.getAttribute('type') || '').toLowerCase();
            if (type === 'checkbox' || type === 'radio') {
                if (!el.checked) { continue; }
            }
            if (type === 'submit' || type === 'reset' || type === 'button' || type === 'image') { continue; }
            this._entries.push([String(name), String(el.value || '')]);
        }
    }
}

FormData.prototype.append = function(name, value) {
    this._entries.push([String(name), String(value)]);
};

FormData.prototype.delete = function(name) {
    var n = String(name);
    this._entries = this._entries.filter(function(e) { return e[0] !== n; });
};

FormData.prototype.get = function(name) {
    var n = String(name);
    for (var i = 0; i < this._entries.length; i++) {
        if (this._entries[i][0] === n) { return this._entries[i][1]; }
    }
    return null;
};

FormData.prototype.getAll = function(name) {
    var n = String(name);
    return this._entries.filter(function(e) { return e[0] === n; }).map(function(e) { return e[1]; });
};

FormData.prototype.has = function(name) {
    var n = String(name);
    return this._entries.some(function(e) { return e[0] === n; });
};

FormData.prototype.set = function(name, value) {
    var n = String(name), v = String(value);
    var found = false;
    this._entries = this._entries.filter(function(e) {
        if (e[0] === n) {
            if (!found) { found = true; e[1] = v; return true; }
            return false;
        }
        return true;
    });
    if (!found) { this._entries.push([n, v]); }
};

FormData.prototype.entries = function() {
    var arr = this._entries.slice();
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.keys = function() {
    var arr = this._entries.map(function(e) { return e[0]; });
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.values = function() {
    var arr = this._entries.map(function(e) { return e[1]; });
    var i = 0;
    return {
        next: function() {
            if (i < arr.length) { return { value: arr[i++], done: false }; }
            return { value: undefined, done: true };
        },
        [Symbol.iterator]: function() { return this; }
    };
};

FormData.prototype.forEach = function(cb, thisArg) {
    for (var i = 0; i < this._entries.length; i++) {
        cb.call(thisArg, this._entries[i][1], this._entries[i][0], this);
    }
};

FormData.prototype[Symbol.iterator] = function() { return this.entries(); };

/// Serialize to application/x-www-form-urlencoded (RFC 3986 percent-encoding).
FormData.prototype._toUrlEncoded = function() {
    return this._entries.map(function(e) {
        return encodeURIComponent(e[0]) + '=' + encodeURIComponent(e[1]);
    }).join('&');
};

FormData.prototype._toMultipart = function(boundary) {
    var enc = new TextEncoder();
    var parts = [];
    var dash = enc.encode('--');
    var bnd = enc.encode(boundary);
    var crlf = enc.encode('\r\n');
    for (var i = 0; i < this._entries.length; i++) {
        var name = this._entries[i][0];
        var value = this._entries[i][1];
        var safeName = name.replace(/\r/g, '%0D').replace(/\n/g, '%0A').replace(/\x22/g, '%22');
        var disp = 'Content-Disposition: form-data; name=\x22' + safeName + '\x22\r\n\r\n';
        var dispHeader = enc.encode(disp);
        var body = enc.encode(value);
        parts.push(dash, bnd, crlf, dispHeader, body, crlf);
    }
    parts.push(dash, bnd, enc.encode('--'), crlf);
    var totalLen = 0;
    for (var j = 0; j < parts.length; j++) { totalLen += parts[j].length; }
    var out = new Uint8Array(totalLen);
    var off = 0;
    for (var k = 0; k < parts.length; k++) {
        out.set(parts[k], off);
        off += parts[k].length;
    }
    return out;
};

