// WHATWG File API §24.9 — URL.createObjectURL / revokeObjectURL
var _object_url_store = Object.create(null);
var _object_url_seq   = 0;
URL.createObjectURL = function(blob) {
    var key = 'blob:lumen/' + (++_object_url_seq);
    _object_url_store[key] = blob;
    return key;
};
URL.revokeObjectURL = function(url) { delete _object_url_store[String(url)]; };
