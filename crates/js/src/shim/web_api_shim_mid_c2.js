// WHATWG File API §24.9 — URL.createObjectURL / revokeObjectURL
var _object_url_store = Object.create(null);
// File API §8.3 «generate a new blob URL» (BUG-1197): `blob:` + the serialized
// origin of the creating document + `/` + a UUID. The origin travels inside the
// URL — the URL Standard derives a blob: URL's origin from the URL in its path,
// so `new URL(u).origin`, a blob: worker's `location.origin` and
// `Origin.from()` inside that worker all see the page's tuple origin. An opaque
// document origin serializes as `null`.
function _lumen_blob_url_uuid() {
    if (typeof crypto !== 'undefined' && crypto && typeof crypto.randomUUID === 'function') {
        return crypto.randomUUID();
    }
    return 'xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx'.replace(/[xy]/g, function(c) {
        var r = Math.random() * 16 | 0;
        return (c === 'x' ? r : (r & 0x3 | 0x8)).toString(16);
    });
}
URL.createObjectURL = function(blob) {
    var origin = (typeof location !== 'undefined' && location) ? String(location.origin || '') : '';
    var key = 'blob:' + (origin || 'null') + '/' + _lumen_blob_url_uuid();
    _object_url_store[key] = blob;
    return key;
};
URL.revokeObjectURL = function(url) { delete _object_url_store[String(url)]; };
// File API §8.3 «resolve a blob URL»: the store is keyed by the URL without its
// fragment, so `blob:<origin>/<uuid>#frag` names the same entry. A revoked or never
// registered URL — and anything that is not a Blob of ours — resolves to null,
// which Fetch §4.2 «scheme fetch» turns into a network error (BUG-1126).
function _lumen_blob_url_entry(url) {
    var key = String(url);
    var hash = key.indexOf('#');
    if (hash !== -1) key = key.slice(0, hash);
    var blob = _object_url_store[key];
    return (blob && blob._bytes) ? blob : null;
}
