// WHATWG File API §24.9 — URL.createObjectURL / revokeObjectURL
var _object_url_store = Object.create(null);
var _object_url_seq   = 0;
URL.createObjectURL = function(blob) {
    var key = 'blob:lumen/' + (++_object_url_seq);
    _object_url_store[key] = blob;
    return key;
};
URL.revokeObjectURL = function(url) { delete _object_url_store[String(url)]; };
// File API §8.3 «resolve a blob URL»: the store is keyed by the URL without its
// fragment, so `blob:lumen/1#frag` names the same entry. A revoked or never
// registered URL — and anything that is not a Blob of ours — resolves to null,
// which Fetch §4.2 «scheme fetch» turns into a network error (BUG-1126).
function _lumen_blob_url_entry(url) {
    var key = String(url);
    var hash = key.indexOf('#');
    if (hash !== -1) key = key.slice(0, hash);
    var blob = _object_url_store[key];
    return (blob && blob._bytes) ? blob : null;
}
