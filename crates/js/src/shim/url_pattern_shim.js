(function() {
  'use strict';

  // URLPattern Standard (https://urlpattern.spec.whatwg.org/): tokenizer,
  // pattern parser, regexp/pattern-string generation, constructor-string
  // parser, init processing, matching, compareComponent and generate().

  var COMPONENTS = ['protocol', 'username', 'password', 'hostname', 'port',
                    'pathname', 'search', 'hash'];
  var SPECIAL_SCHEMES = ['ftp', 'file', 'http', 'https', 'ws', 'wss'];
  var DEFAULT_PORTS = {ftp: '21', http: '80', https: '443', ws: '80', wss: '443'};

  function fail(msg) { throw new TypeError('URLPattern: ' + msg); }

  // ---- Tokenizer ---------------------------------------------------------

  var ID_START = /^[\p{ID_Start}$_]$/u;
  var ID_CONT = new RegExp('^[\\p{ID_Continue}$' + String.fromCharCode(0x200c, 0x200d) + ']$', 'u');

  function isValidNameCodePoint(cp, first) {
    return first ? ID_START.test(cp) : ID_CONT.test(cp);
  }

  function isAscii(cp) { return cp.codePointAt(0) <= 0x7f; }

  function tokenize(cps, strict) {
    var tokens = [];
    var i = 0;
    var n = cps.length;
    function add(type, nextPos, valuePos) {
      tokens.push({type: type, index: i, value: cps.slice(valuePos, nextPos).join('')});
      i = nextPos;
    }
    function error(nextPos, valuePos) {
      if (strict) fail('invalid pattern');
      add('INVALID_CHAR', nextPos, valuePos);
    }
    while (i < n) {
      var c = cps[i];
      if (c === '*') { add('ASTERISK', i + 1, i); continue; }
      if (c === '+' || c === '?') { add('OTHER_MODIFIER', i + 1, i); continue; }
      if (c === '\\') {
        if (i === n - 1) { error(i + 1, i); continue; }
        var at = i;
        tokens.push({type: 'ESCAPED_CHAR', index: at, value: cps[i + 1]});
        i += 2;
        continue;
      }
      if (c === '{') { add('OPEN', i + 1, i); continue; }
      if (c === '}') { add('CLOSE', i + 1, i); continue; }
      if (c === ':') {
        var np = i + 1;
        var name = '';
        while (np < n && isValidNameCodePoint(cps[np], np === i + 1)) {
          name += cps[np];
          np++;
        }
        if (name === '') { error(i + 1, i); continue; }
        tokens.push({type: 'NAME', index: i, value: name});
        i = np;
        continue;
      }
      if (c === '(') {
        var depth = 1;
        var p = i + 1;
        var re = '';
        var bad = false;
        while (p < n) {
          var ch = cps[p];
          if (!isAscii(ch)) { bad = true; break; }
          if (p === i + 1 && ch === '?') { bad = true; break; }
          if (ch === '\\') {
            if (p === n - 1) { bad = true; break; }
            var nx = cps[p + 1];
            if (!isAscii(nx)) { bad = true; break; }
            re += ch + nx;
            p += 2;
            continue;
          }
          if (ch === ')') {
            depth--;
            if (depth === 0) { p++; break; }
          } else if (ch === '(') {
            depth++;
            if (p === n - 1 || cps[p + 1] !== '?') { bad = true; break; }
          }
          re += ch;
          p++;
        }
        if (!bad && (depth !== 0 || p > n || re === '')) bad = true;
        if (bad) { error(i + 1, i); continue; }
        tokens.push({type: 'REGEXP', index: i, value: re});
        i = p;
        continue;
      }
      add('CHAR', i + 1, i);
    }
    tokens.push({type: 'END', index: n, value: ''});
    return tokens;
  }

  // ---- Pattern string parser --------------------------------------------

  function escapeRegexp(s) { return s.replace(/[.+*?^${}()|[\]\\\/]/g, '\\$&'); }
  function escapePattern(s) { return s.replace(/[+*?:{}()\\]/g, '\\$&'); }

  var MOD_STR = {'none': '', 'optional': '?', 'one-or-more': '+', 'zero-or-more': '*'};

  function parsePatternString(input, options, encode) {
    var tokens = tokenize(Array.from(input), true);
    var segRe = '[^' + escapeRegexp(options.delimiter) + ']+?';
    var parts = [];
    var pending = '';
    var index = 0;
    var nextNumeric = 0;

    function tryConsume(type) {
      if (tokens[index].type === type) return tokens[index++];
      return null;
    }
    function tryConsumeModifier() {
      return tryConsume('OTHER_MODIFIER') || tryConsume('ASTERISK');
    }
    function consumeRequired(type) {
      var t = tryConsume(type);
      if (!t) fail('unexpected token ' + tokens[index].type + ', expected ' + type);
      return t;
    }
    function consumeText() {
      var r = '';
      for (;;) {
        var t = tryConsume('CHAR') || tryConsume('ESCAPED_CHAR');
        if (!t) break;
        r += t.value;
      }
      return r;
    }
    function isDuplicate(name) {
      for (var k = 0; k < parts.length; k++) if (parts[k].name === name) return true;
      return false;
    }
    function flushPending() {
      if (pending === '') return;
      var v = encode(pending);
      pending = '';
      parts.push({type: 'fixed-text', name: '', prefix: '', value: v, suffix: '', modifier: 'none'});
    }
    function addPart(prefix, nameTok, reTok, suffix, modTok) {
      var modifier = 'none';
      if (modTok) {
        modifier = modTok.value === '?' ? 'optional'
          : modTok.value === '*' ? 'zero-or-more' : 'one-or-more';
      }
      if (!nameTok && !reTok && modifier === 'none') { pending += prefix; return; }
      flushPending();
      if (!nameTok && !reTok) {
        if (prefix === '') return;
        parts.push({type: 'fixed-text', name: '', prefix: '', value: encode(prefix),
                    suffix: '', modifier: modifier});
        return;
      }
      var reValue;
      if (!reTok) reValue = segRe;
      else if (reTok.type === 'ASTERISK') reValue = '.*';
      else reValue = reTok.value;
      var type = 'regexp';
      if (reValue === segRe) { type = 'segment-wildcard'; reValue = ''; }
      else if (reValue === '.*') { type = 'full-wildcard'; reValue = ''; }
      var name;
      if (nameTok) name = nameTok.value;
      else name = String(nextNumeric++);
      if (isDuplicate(name)) fail('duplicate group name ' + name);
      parts.push({type: type, name: name, prefix: encode(prefix), value: reValue,
                  suffix: encode(suffix), modifier: modifier});
    }

    while (index < tokens.length) {
      var charTok = tryConsume('CHAR');
      var nameTok = tryConsume('NAME');
      var reTok = tryConsume('REGEXP');
      if (!nameTok && !reTok) reTok = tryConsume('ASTERISK');
      if (nameTok || reTok) {
        var prefix = charTok ? charTok.value : '';
        if (prefix !== '' && prefix !== options.prefix) {
          pending += prefix;
          prefix = '';
        }
        flushPending();
        addPart(prefix, nameTok, reTok, '', tryConsumeModifier());
        continue;
      }
      var fixedTok = charTok || tryConsume('ESCAPED_CHAR');
      if (fixedTok) { pending += fixedTok.value; continue; }
      if (tryConsume('OPEN')) {
        var pre = consumeText();
        var nt = tryConsume('NAME');
        var rt = tryConsume('REGEXP');
        if (!nt && !rt) rt = tryConsume('ASTERISK');
        var suf = consumeText();
        consumeRequired('CLOSE');
        addPart(pre, nt, rt, suf, tryConsumeModifier());
        continue;
      }
      flushPending();
      consumeRequired('END');
    }
    return parts;
  }

  function generateRegexp(parts, options) {
    var segRe = '[^' + escapeRegexp(options.delimiter) + ']+?';
    var result = '^';
    var names = [];
    for (var k = 0; k < parts.length; k++) {
      var p = parts[k];
      var mod = MOD_STR[p.modifier];
      if (p.type === 'fixed-text') {
        if (p.modifier === 'none') result += escapeRegexp(p.value);
        else result += '(?:' + escapeRegexp(p.value) + ')' + mod;
        continue;
      }
      var re = p.type === 'regexp' ? p.value : p.type === 'segment-wildcard' ? segRe : '.*';
      names.push(p.name);
      if (p.prefix === '' && p.suffix === '') {
        if (p.modifier === 'none' || p.modifier === 'optional') {
          result += '(' + re + ')' + mod;
        } else {
          result += '((?:' + re + ')' + mod + ')';
        }
        continue;
      }
      var ep = escapeRegexp(p.prefix);
      var es = escapeRegexp(p.suffix);
      if (p.modifier === 'none' || p.modifier === 'optional') {
        result += '(?:' + ep + '(' + re + ')' + es + ')' + mod;
        continue;
      }
      result += '(?:' + ep + '((?:' + re + ')(?:' + es + ep + '(?:' + re + '))*)' + es + ')';
      if (p.modifier === 'zero-or-more') result += '?';
    }
    result += '$';
    return {source: result, names: names};
  }

  function generatePatternString(parts, options) {
    var segRe = '[^' + escapeRegexp(options.delimiter) + ']+?';
    var result = '';
    for (var k = 0; k < parts.length; k++) {
      var p = parts[k];
      var prev = k > 0 ? parts[k - 1] : null;
      var next = k < parts.length - 1 ? parts[k + 1] : null;
      var mod = MOD_STR[p.modifier];
      if (p.type === 'fixed-text') {
        if (p.modifier === 'none') result += escapePattern(p.value);
        else result += '{' + escapePattern(p.value) + '}' + mod;
        continue;
      }
      var custom = !/^[0-9]/.test(p.name);
      var grouping = p.suffix !== '' || (p.prefix !== '' && p.prefix !== options.prefix);
      if (!grouping && custom && p.type === 'segment-wildcard' && p.modifier === 'none' &&
          next !== null && next.prefix === '' && next.suffix === '') {
        if (next.type === 'fixed-text') {
          grouping = isValidNameCodePoint(Array.from(next.value)[0], false);
        } else {
          grouping = /^[0-9]/.test(next.name);
        }
      }
      if (!grouping && p.prefix === '' && prev !== null && prev.type === 'fixed-text' &&
          prev.value !== '' && options.prefix !== '') {
        var pv = Array.from(prev.value);
        if (pv[pv.length - 1] === options.prefix) grouping = true;
      }
      if (grouping) result += '{';
      result += escapePattern(p.prefix);
      if (custom) result += ':' + p.name;
      if (p.type === 'regexp') {
        result += '(' + p.value + ')';
      } else if (p.type === 'segment-wildcard') {
        if (!custom) result += '(' + segRe + ')';
      } else {
        if (!custom && (prev === null || prev.type === 'fixed-text' || prev.modifier !== 'none' ||
                        grouping || p.prefix !== '')) {
          result += '*';
        } else {
          result += '(.*)';
        }
      }
      if (p.type === 'segment-wildcard' && custom && p.suffix !== '' &&
          isValidNameCodePoint(Array.from(p.suffix)[0], false)) {
        result += '\\';
      }
      result += escapePattern(p.suffix);
      if (grouping) result += '}';
      result += mod;
    }
    return result;
  }

  // ---- Canonicalization --------------------------------------------------

  // Percent-encode every code point that is C0/non-ASCII or listed in `extra`
  // (the spec's percent-encode sets are built from these). The page's URL
  // setters are not used: they neither encode nor reject the way the URL
  // Standard's state-override parsers do.
  function pctEncode(s, extra) {
    var out = '';
    var cps = Array.from(s);
    for (var k = 0; k < cps.length; k++) {
      var cp = cps[k];
      var code = cp.codePointAt(0);
      if (code < 0x20 || code > 0x7e || extra.indexOf(cp) !== -1) out += utf8Encode(cp);
      else out += cp;
    }
    return out;
  }

  function utf8Encode(cp) {
    // Percent-encode one code point (lone surrogates become U+FFFD).
    try { return encodeURIComponent(cp).replace(/[!'()*]/g, function(ch) {
      return '%' + ch.charCodeAt(0).toString(16).toUpperCase();
    }); } catch (e) { return '%EF%BF%BD'; }
  }

  var FRAGMENT_SET = ' "<>`';
  var SPECIAL_QUERY_SET = ' "#<>\'';
  var USERINFO_SET = ' "#<>?`{}/:;=@[\\]^|';

  function canonProtocol(v) {
    if (v === '') return v;
    var u;
    try { u = new URL(v + '://dummy.test'); } catch (e) { fail('invalid protocol'); }
    return u.protocol.slice(0, -1);
  }
  function canonUsername(v) { return pctEncode(v, USERINFO_SET); }
  function canonPassword(v) { return pctEncode(v, USERINFO_SET); }
  function canonHostname(v) {
    if (v === '') return v;
    v = v.replace(/[\t\n\r]/g, '');
    // Host state with an override stops at the end of the authority.
    var end = v.search(/[\/?#\\]/);
    var host = end === -1 ? v : v.slice(0, end);
    if (host.charAt(0) !== '[') {
      try { host = decodeURIComponent(host); } catch (e) { fail('invalid hostname'); }
      if (/[\u0000- #%\/:<>?@\[\\\]^|\u007f]/.test(host)) fail('invalid hostname');
    } else if (!/^\[[0-9a-fA-F:.]*\]$/.test(host)) {
      fail('invalid hostname');
    }
    if (host === '') fail('invalid hostname');
    try { return new URL('https://' + host + '/').hostname; } catch (e) { fail('invalid hostname'); }
  }
  function canonIPv6(v) {
    if (v === '') return v;
    if (!/^[0-9a-fA-F:.\[\]]*$/.test(v)) fail('invalid IPv6 hostname');
    return v.toLowerCase();
  }
  function canonPort(v, protocol) {
    if (v === '') return v;
    v = v.replace(/[\t\n\r]/g, '');
    var m = /^[0-9]+/.exec(v);
    if (!m) fail('invalid port');
    var num = parseInt(m[0], 10);
    if (num > 65535) fail('invalid port');
    var s = String(num);
    if (protocol && DEFAULT_PORTS[protocol] === s) return '';
    return s;
  }
  function canonPathname(v) {
    if (v === '') return v;
    var lead = v.charAt(0) === '/';
    var modified = (lead ? v : '/-' + v).split('?').join('%3F').split('#').join('%23');
    // The trailing `?x` keeps the parser from trimming a trailing space.
    var p = new URL('https://dummy.test' + modified + '?x').pathname;
    return lead ? p : p.slice(2);
  }
  function canonOpaquePathname(v) {
    if (v === '') return v;
    return pctEncode(v, '');
  }
  function canonSearch(v) {
    return pctEncode(v, SPECIAL_QUERY_SET);
  }
  function canonHash(v) {
    return pctEncode(v, FRAGMENT_SET);
  }

  function usv(s) {
    s = String(s);
    return typeof s.toWellFormed === 'function' ? s.toWellFormed() : s;
  }

  // ---- URLPatternInit processing ----------------------------------------

  function isAbsolutePathname(v, type) {
    if (v === '') return false;
    if (v.charAt(0) === '/') return true;
    if (type === 'url') return false;
    if (v.length < 2) return false;
    return (v.charAt(0) === '\\' || v.charAt(0) === '{') && v.charAt(1) === '/';
  }

  function hasOpaquePath(u) {
    return u.pathname.charAt(0) !== '/' && u.href.indexOf(u.protocol + '//') !== 0;
  }

  function processInit(init, type, defaults) {
    var r = {};
    COMPONENTS.forEach(function(c) { r[c] = defaults ? defaults[c] : undefined; });
    var baseURL = null;
    if (init.baseURL !== undefined) {
      try { baseURL = new URL(init.baseURL); } catch (e) { fail('invalid baseURL'); }
      var esc = type === 'pattern' ? escapePattern : function(x) { return x; };
      if (init.protocol === undefined) r.protocol = esc(baseURL.protocol.slice(0, -1));
      if (type !== 'pattern' && init.protocol === undefined && init.hostname === undefined &&
          init.port === undefined && init.username === undefined) {
        r.username = baseURL.username;
      }
      if (type !== 'pattern' && init.protocol === undefined && init.hostname === undefined &&
          init.port === undefined && init.username === undefined && init.password === undefined) {
        r.password = baseURL.password;
      }
      if (init.protocol === undefined && init.hostname === undefined) r.hostname = esc(baseURL.hostname);
      if (init.protocol === undefined && init.hostname === undefined && init.port === undefined) {
        r.port = esc(baseURL.port);
      }
      if (init.protocol === undefined && init.hostname === undefined && init.port === undefined &&
          init.pathname === undefined) {
        r.pathname = esc(baseURL.pathname);
      }
      if (init.protocol === undefined && init.hostname === undefined && init.port === undefined &&
          init.pathname === undefined && init.search === undefined) {
        r.search = esc(baseURL.search.replace(/^\?/, ''));
      }
      if (init.protocol === undefined && init.hostname === undefined && init.port === undefined &&
          init.pathname === undefined && init.search === undefined && init.hash === undefined) {
        r.hash = esc(baseURL.hash.replace(/^#/, ''));
      }
    }
    if (init.protocol !== undefined) {
      var pv = init.protocol.charAt(init.protocol.length - 1) === ':'
        ? init.protocol.slice(0, -1) : init.protocol;
      r.protocol = type === 'pattern' ? pv : canonProtocol(pv);
    }
    if (init.username !== undefined) {
      r.username = type === 'pattern' ? init.username : canonUsername(init.username);
    }
    if (init.password !== undefined) {
      r.password = type === 'pattern' ? init.password : canonPassword(init.password);
    }
    if (init.hostname !== undefined) {
      r.hostname = type === 'pattern' ? init.hostname : canonHostname(init.hostname);
    }
    if (init.port !== undefined) {
      r.port = type === 'pattern' ? init.port : canonPort(init.port, r.protocol);
    }
    if (init.pathname !== undefined) {
      r.pathname = init.pathname;
      if (baseURL && !hasOpaquePath(baseURL) && !isAbsolutePathname(r.pathname, type)) {
        var bp = baseURL.pathname;
        var slash = bp.lastIndexOf('/');
        if (slash !== -1) r.pathname = bp.slice(0, slash + 1) + r.pathname;
      }
      if (type !== 'pattern') {
        var proto = r.protocol;
        r.pathname = (proto === undefined || proto === '' || SPECIAL_SCHEMES.indexOf(proto) !== -1)
          ? canonPathname(r.pathname) : canonOpaquePathname(r.pathname);
      }
    }
    if (init.search !== undefined) {
      var sv = init.search.charAt(0) === '?' ? init.search.slice(1) : init.search;
      r.search = type === 'pattern' ? sv : canonSearch(sv);
    }
    if (init.hash !== undefined) {
      var hv = init.hash.charAt(0) === '#' ? init.hash.slice(1) : init.hash;
      r.hash = type === 'pattern' ? hv : canonHash(hv);
    }
    return r;
  }

  // ---- Constructor string parser ----------------------------------------

  function parseConstructorString(input) {
    var cps = Array.from(input);
    var tokens = tokenize(cps, false);
    var result = {};
    var componentStart = 0;
    var tokenIndex = 0;
    var tokenIncrement = 1;
    var groupDepth = 0;
    var ipv6Depth = 0;
    var specialFlag = false;
    var state = 'init';

    function safeToken(i) { return i < tokens.length ? tokens[i] : tokens[tokens.length - 1]; }
    function isNonSpecial(i, v) {
      var t = safeToken(i);
      return t.value === v && (t.type === 'CHAR' || t.type === 'ESCAPED_CHAR' || t.type === 'INVALID_CHAR');
    }
    function isSearchPrefix() {
      if (isNonSpecial(tokenIndex, '?')) return true;
      if (tokens[tokenIndex].value !== '?') return false;
      var pi = tokenIndex - 1;
      if (pi < 0) return true;
      var pt = tokens[pi].type;
      return !(pt === 'NAME' || pt === 'REGEXP' || pt === 'CLOSE' || pt === 'ASTERISK');
    }
    function makeComponentString() {
      var s = safeToken(componentStart).index;
      var e = tokens[tokenIndex].index;
      return cps.slice(s, e).join('');
    }
    function changeState(newState, skip) {
      if (state !== 'init' && state !== 'authority' && state !== 'done') {
        result[state] = makeComponentString();
      }
      if (state !== 'init' && newState !== 'done') {
        var early = ['protocol', 'authority', 'username', 'password'];
        var mid = early.concat(['hostname', 'port']);
        var late = mid.concat(['pathname']);
        if (early.indexOf(state) !== -1 && ['port', 'pathname', 'search', 'hash'].indexOf(newState) !== -1 &&
            result.hostname === undefined) {
          result.hostname = '';
        }
        if (mid.indexOf(state) !== -1 && (newState === 'search' || newState === 'hash') &&
            result.pathname === undefined) {
          result.pathname = specialFlag ? '/' : '';
        }
        if (late.indexOf(state) !== -1 && newState === 'hash' && result.search === undefined) {
          result.search = '';
        }
      }
      state = newState;
      componentStart = tokenIndex + skip;
      tokenIndex += skip;
      tokenIncrement = 0;
    }
    function rewind() { tokenIndex = componentStart; tokenIncrement = 0; }
    function rewindAndSetState(s) { rewind(); state = s; }
    function pathStartOrMore() {
      return isNonSpecial(tokenIndex, '/') || isSearchPrefix() || isNonSpecial(tokenIndex, '#');
    }

    for (; tokenIndex < tokens.length; tokenIndex += tokenIncrement) {
      tokenIncrement = 1;
      if (tokens[tokenIndex].type === 'END') {
        if (state === 'init') {
          rewind();
          if (isNonSpecial(tokenIndex, '#')) changeState('hash', 1);
          else if (isSearchPrefix()) changeState('search', 1);
          else changeState('pathname', 0);
          continue;
        }
        if (state === 'authority') { rewindAndSetState('hostname'); continue; }
        changeState('done', 0);
        break;
      }
      if (tokens[tokenIndex].type === 'OPEN') { groupDepth++; continue; }
      if (groupDepth > 0) {
        if (tokens[tokenIndex].type === 'CLOSE') groupDepth--;
        else continue;
      }
      switch (state) {
        case 'init':
          if (isNonSpecial(tokenIndex, ':')) rewindAndSetState('protocol');
          break;
        case 'protocol':
          if (isNonSpecial(tokenIndex, ':')) {
            var protoStr = makeComponentString();
            var pc = compileComponent(protoStr, canonProtocol, DEFAULT_OPTIONS, false);
            specialFlag = matchesSpecialScheme(pc);
            var nextState = 'pathname';
            var skip = 1;
            if (isNonSpecial(tokenIndex + 1, '/') && isNonSpecial(tokenIndex + 2, '/')) {
              nextState = 'authority';
              skip = 3;
            } else if (specialFlag) {
              nextState = 'authority';
            }
            changeState(nextState, skip);
          }
          break;
        case 'authority':
          if (isNonSpecial(tokenIndex, '@')) rewindAndSetState('username');
          else if (pathStartOrMore()) rewindAndSetState('hostname');
          break;
        case 'username':
          if (isNonSpecial(tokenIndex, ':')) changeState('password', 1);
          else if (isNonSpecial(tokenIndex, '@')) changeState('hostname', 1);
          else if (pathStartOrMore()) changeState('hostname', 0);
          break;
        case 'password':
          if (isNonSpecial(tokenIndex, '@')) changeState('hostname', 1);
          else if (pathStartOrMore()) changeState('hostname', 0);
          break;
        case 'hostname':
          if (isNonSpecial(tokenIndex, '[')) ipv6Depth++;
          else if (isNonSpecial(tokenIndex, ']')) ipv6Depth--;
          else if (isNonSpecial(tokenIndex, ':') && ipv6Depth === 0) changeState('port', 1);
          else if (isNonSpecial(tokenIndex, '/')) changeState('pathname', 0);
          else if (isSearchPrefix()) changeState('search', 1);
          else if (isNonSpecial(tokenIndex, '#')) changeState('hash', 1);
          break;
        case 'port':
          if (isNonSpecial(tokenIndex, '/')) changeState('pathname', 0);
          else if (isSearchPrefix()) changeState('search', 1);
          else if (isNonSpecial(tokenIndex, '#')) changeState('hash', 1);
          break;
        case 'pathname':
          if (isSearchPrefix()) changeState('search', 1);
          else if (isNonSpecial(tokenIndex, '#')) changeState('hash', 1);
          break;
        case 'search':
          if (isNonSpecial(tokenIndex, '#')) changeState('hash', 1);
          break;
        default:
          break;
      }
    }
    if (result.hostname !== undefined && result.port === undefined) result.port = '';
    return result;
  }

  // ---- Components --------------------------------------------------------

  var DEFAULT_OPTIONS = {delimiter: '', prefix: ''};
  var HOSTNAME_OPTIONS = {delimiter: '.', prefix: ''};
  var PATHNAME_OPTIONS = {delimiter: '/', prefix: '/'};

  function compileComponent(input, encode, options, ignoreCase) {
    var parts = parsePatternString(input, options, encode);
    var gen = generateRegexp(parts, options);
    var re;
    try { re = new RegExp(gen.source, ignoreCase ? 'vi' : 'v'); } catch (e) { fail('invalid regular expression'); }
    var hasRegexp = false;
    for (var k = 0; k < parts.length; k++) if (parts[k].type === 'regexp') hasRegexp = true;
    return {
      pattern: generatePatternString(parts, options),
      regexp: re,
      names: gen.names,
      parts: parts,
      options: options,
      encode: encode,
      hasRegexpGroups: hasRegexp
    };
  }

  function matchesSpecialScheme(protocolComponent) {
    for (var k = 0; k < SPECIAL_SCHEMES.length; k++) {
      if (protocolComponent.regexp.test(SPECIAL_SCHEMES[k])) return true;
    }
    return false;
  }

  function isIPv6Pattern(s) {
    if (s.length < 2) return false;
    if (s.charAt(0) === '[') return true;
    return (s.charAt(0) === '{' || s.charAt(0) === '\\') && s.charAt(1) === '[';
  }

  // ---- WebIDL-ish argument conversion -----------------------------------

  var INIT_KEYS = COMPONENTS.concat(['baseURL']);

  function toInit(obj) {
    var init = {};
    for (var k = 0; k < INIT_KEYS.length; k++) {
      var v = obj[INIT_KEYS[k]];
      if (v !== undefined) init[INIT_KEYS[k]] = usv(v);
    }
    return init;
  }

  var internals = new WeakMap();

  function getInternals(o) {
    var s = internals.get(o);
    if (!s) throw new TypeError('Illegal invocation');
    return s;
  }

  class URLPattern {
    constructor(input, baseOrOptions, maybeOptions) {
      var baseURL;
      var options;
      if (typeof baseOrOptions === 'string') {
        baseURL = usv(baseOrOptions);
        options = maybeOptions;
      } else {
        options = baseOrOptions;
        if (baseOrOptions !== undefined && baseOrOptions !== null && typeof baseOrOptions !== 'object' &&
            typeof baseOrOptions !== 'function') {
          baseURL = usv(baseOrOptions);
          options = maybeOptions;
        }
      }
      var ignoreCase = !!(options !== undefined && options !== null && options.ignoreCase);
      var init;
      if (input === undefined) input = {};
      if (typeof input === 'object' && input !== null || typeof input === 'function') {
        if (baseURL !== undefined) fail('baseURL cannot be combined with an init dictionary');
        init = toInit(input);
      } else {
        init = parseConstructorString(usv(input));
        if (baseURL === undefined && init.protocol === undefined) {
          fail('relative constructor string requires a baseURL');
        }
        if (baseURL !== undefined) init.baseURL = baseURL;
      }
      var p = processInit(init, 'pattern', null);
      COMPONENTS.forEach(function(c) { if (p[c] === undefined) p[c] = '*'; });
      if (DEFAULT_PORTS[p.protocol] === p.port) p.port = '';
      var c = {};
      c.protocol = compileComponent(p.protocol, canonProtocol, DEFAULT_OPTIONS, ignoreCase);
      c.username = compileComponent(p.username, canonUsername, DEFAULT_OPTIONS, ignoreCase);
      c.password = compileComponent(p.password, canonPassword, DEFAULT_OPTIONS, ignoreCase);
      c.hostname = isIPv6Pattern(p.hostname)
        ? compileComponent(p.hostname, canonIPv6, HOSTNAME_OPTIONS, ignoreCase)
        : compileComponent(p.hostname, canonHostname, HOSTNAME_OPTIONS, ignoreCase);
      c.port = compileComponent(p.port, function(v) { return canonPort(v, null); }, DEFAULT_OPTIONS, ignoreCase);
      if (matchesSpecialScheme(c.protocol)) {
        c.pathname = compileComponent(p.pathname, canonPathname, PATHNAME_OPTIONS, ignoreCase);
      } else {
        c.pathname = compileComponent(p.pathname, canonOpaquePathname, DEFAULT_OPTIONS, ignoreCase);
      }
      c.search = compileComponent(p.search, canonSearch, DEFAULT_OPTIONS, ignoreCase);
      c.hash = compileComponent(p.hash, canonHash, DEFAULT_OPTIONS, ignoreCase);
      internals.set(this, c);
    }

    test(input, baseURL) {
      return matchPattern(getInternals(this), input, baseURL, false) !== null;
    }

    exec(input, baseURL) {
      return matchPattern(getInternals(this), input, baseURL, true);
    }

    get hasRegExpGroups() {
      var c = getInternals(this);
      return COMPONENTS.some(function(k) { return c[k].hasRegexpGroups; });
    }

    generate(component, groups) {
      var c = getInternals(this);
      component = String(component);
      if (COMPONENTS.indexOf(component) === -1) throw new TypeError('invalid component');
      var comp = c[component];
      if (groups === undefined) groups = {};
      var out = '';
      for (var k = 0; k < comp.parts.length; k++) {
        var p = comp.parts[k];
        if (p.type === 'fixed-text') {
          if (p.modifier !== 'none') throw new TypeError('cannot generate a modified fixed-text part');
          out += p.value;
          continue;
        }
        if (p.type !== 'segment-wildcard') throw new TypeError('cannot generate a non-segment group');
        var v = groups[p.name];
        if (v === undefined) {
          if (p.modifier === 'optional' || p.modifier === 'zero-or-more') continue;
          throw new TypeError('missing group ' + p.name);
        }
        var enc = comp.encode(String(v));
        var segRe = new RegExp('^[^' + escapeRegexp(comp.options.delimiter) + ']+?$', 'u');
        if (!segRe.test(enc)) throw new TypeError('group value does not match');
        out += p.prefix + enc + p.suffix;
      }
      return out;
    }

    static compareComponent(component, left, right) {
      component = String(component);
      if (COMPONENTS.indexOf(component) === -1) throw new TypeError('invalid component');
      if (!(left instanceof URLPattern) || !(right instanceof URLPattern)) {
        throw new TypeError('URLPattern expected');
      }
      return compareParts(getInternals(left)[component].parts, getInternals(right)[component].parts);
    }
  }

  var TYPE_RANK = {'fixed-text': 3, 'regexp': 2, 'segment-wildcard': 1, 'full-wildcard': 0};
  var MOD_RANK = {'none': 3, 'one-or-more': 2, 'optional': 1, 'zero-or-more': 0};

  function cmp(a, b) { return a < b ? -1 : a > b ? 1 : 0; }

  function compareParts(l, r) {
    for (var k = 0; ; k++) {
      var a = l[k];
      var b = r[k];
      if (!a && !b) return 0;
      if (!a) return -1;
      if (!b) return 1;
      var res = cmp(TYPE_RANK[a.type], TYPE_RANK[b.type]);
      if (res === 0) res = cmp(MOD_RANK[a.modifier], MOD_RANK[b.modifier]);
      if (res === 0) res = cmp(a.prefix, b.prefix);
      if (res === 0) res = cmp(a.value, b.value);
      if (res === 0) res = cmp(a.suffix, b.suffix);
      if (res !== 0) return res;
    }
  }

  function matchPattern(c, input, baseURL, wantResult) {
    var inputs = [];
    var v = {};
    if (input === undefined) input = {};
    if (typeof input === 'object' && input !== null || typeof input === 'function') {
      if (baseURL !== undefined) throw new TypeError('baseURL cannot be combined with an init dictionary');
      inputs.push(input);
      var empty = {};
      COMPONENTS.forEach(function(k) { empty[k] = ''; });
      try { v = processInit(toInit(input), 'url', empty); } catch (e) { return null; }
    } else {
      var s = usv(input);
      var url;
      try {
        if (baseURL !== undefined) {
          baseURL = usv(baseURL);
          url = new URL(s, baseURL);
        } else {
          url = new URL(s);
        }
      } catch (e) { return null; }
      inputs.push(s);
      if (baseURL !== undefined) inputs.push(baseURL);
      v = {
        protocol: url.protocol.slice(0, -1),
        username: url.username,
        password: url.password,
        hostname: url.hostname,
        port: url.port,
        pathname: url.pathname,
        search: url.search.replace(/^\?/, ''),
        hash: url.hash.replace(/^#/, '')
      };
    }
    var result = {inputs: inputs};
    for (var k = 0; k < COMPONENTS.length; k++) {
      var name = COMPONENTS[k];
      var comp = c[name];
      var m = comp.regexp.exec(v[name]);
      if (m === null) return null;
      if (!wantResult) continue;
      var groups = {};
      for (var g = 0; g < comp.names.length; g++) {
        Object.defineProperty(groups, comp.names[g],
          {value: m[g + 1], writable: true, enumerable: true, configurable: true});
      }
      result[name] = {input: v[name], groups: groups};
    }
    return result;
  }

  COMPONENTS.forEach(function(name) {
    Object.defineProperty(URLPattern.prototype, name, {
      get: function() { return getInternals(this)[name].pattern; },
      enumerable: true,
      configurable: true
    });
  });
  Object.defineProperty(URLPattern.prototype, Symbol.toStringTag,
    {value: 'URLPattern', configurable: true});
  Object.defineProperty(globalThis, 'URLPattern',
    {value: URLPattern, writable: true, enumerable: false, configurable: true});
})();
