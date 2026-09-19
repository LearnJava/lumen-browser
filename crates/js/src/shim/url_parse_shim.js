
// _LUMEN_PAGE_URL injected by Rust before this shim runs.
function _lumen_parse_url(url) {
    var href = String(url || '');
    var protocol = '', username = '', password = '';
    var hostname = '', host = '', port = '', pathname = '/', search = '', hash = '', origin = '';
    var sIdx = href.indexOf('://');
    // `hasAuthority` is what tells an authority-bearing URL (`https://h/p`) from
    // an opaque-path one (`mailto:a@b`) when the components are serialized back
    // into an href — the host being empty is not a reliable substitute.
    var hasAuthority = sIdx >= 0;
    if (hasAuthority) {
        protocol = href.slice(0, sIdx + 1);
        var rest = href.slice(sIdx + 3);
        var splitAt = rest.length;
        for (var i = 0; i < rest.length; i++) {
            if (rest[i] === '/' || rest[i] === '?' || rest[i] === '#') { splitAt = i; break; }
        }
        var authority = rest.slice(0, splitAt);
        rest = rest.slice(splitAt);
        // URL Standard §4.4: the *last* '@' ends the userinfo, since '@' itself
        // is legal (percent-encoded aside) inside a username or password.
        var atIdx = authority.lastIndexOf('@');
        if (atIdx >= 0) {
            var creds = authority.slice(0, atIdx);
            authority = authority.slice(atIdx + 1);
            var credColon = creds.indexOf(':');
            username = credColon >= 0 ? creds.slice(0, credColon) : creds;
            password = credColon >= 0 ? creds.slice(credColon + 1) : '';
        }
        var portColon = authority.lastIndexOf(':');
        if (portColon > authority.lastIndexOf(']')) {
            hostname = authority.slice(0, portColon); port = authority.slice(portColon + 1);
        } else {
            hostname = authority; port = '';
        }
        host = port ? hostname + ':' + port : hostname;
        var hIdx = rest.indexOf('#');
        // URL Standard §6.3: an empty fragment/query (nothing after '#'/'?')
        // serializes as '', not as the bare delimiter — a trailing '?' or '#'
        // with no content means "absent", same as never having one.
        if (hIdx >= 0) { hash = rest.slice(hIdx); rest = rest.slice(0, hIdx); if (hash === '#') hash = ''; }
        var qIdx = rest.indexOf('?');
        if (qIdx >= 0) { search = rest.slice(qIdx); rest = rest.slice(0, qIdx); if (search === '?') search = ''; }
        pathname = rest || '/';
        origin = protocol + '//' + host;
    } else {
        var cIdx = href.indexOf(':');
        if (cIdx >= 0) {
            protocol = href.slice(0, cIdx + 1);
            pathname = href.slice(cIdx + 1);
        }
    }
    return { href: href, protocol: protocol, username: username, password: password,
             hostname: hostname, host: host, port: port,
             pathname: pathname, search: search, hash: hash, origin: origin,
             hasAuthority: hasAuthority };
}
