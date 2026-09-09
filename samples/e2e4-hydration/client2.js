// E2E-4, ступень 2: клиентская половина. Дерево повторяет gen2.js.
(function () {
  window.__PROBE = window.__PROBE || [];
  var rep = function (s) { window.__PROBE.push(s); console.log('[probe2] ' + s); };
  window.__rep = rep;
  try { main(); } catch (err) { rep('THROW top: ' + (err && err.stack ? err.stack : err)); }

  function main() {
    var e = React.createElement;

    function Doc() {
      return e(
        'html',
        { lang: 'ru' },
        e(
          'head',
          null,
          e('meta', { charSet: 'utf-8' }),
          e('title', null, 'E2E-4 document hydration'),
        ),
        e(
          'body',
          null,
          e(
            'div',
            { id: 'app' },
            e('h1', null, 'document hydration probe'),
            e(React.Suspense, { fallback: e('p', null, 'loading') }, e('p', { id: 'slow' }, 'suspense payload')),
          ),
          e('script', { src: 'react.js', async: false }),
          e('script', { src: 'react-dom.js', async: false }),
          e('script', { src: 'client2.js', async: false }),
        ),
      );
    }

    // Заглушка-разведка (LOOKAHEAD=1): подставляет отсутствующие методы Node на
    // `document`, чтобы увидеть, ЧТО сломается следующим, а не чинить BUG-557.
    // Семантика заведомо неверная — это разведка, а не полифил.
    // BUG-557 починен 2026-09-09: методы теперь есть, и подстановка ниже сама
    // себя отключает (`typeof !== 'function'`). Оставлено как след итерации 1 —
    // на починенном движке стенд гоняют БЕЗ `?lookahead`.
    if (String(location.search).indexOf('lookahead') >= 0) {
      ['removeChild', 'insertBefore', 'replaceChild'].forEach(function (m) {
        if (typeof document[m] !== 'function') {
          document[m] = function (n) { rep('STUB document.' + m + ' called'); return n; };
        }
      });
    }
    // lookahead=2 добавляет к заглушкам обход: React при гидрации идёт от
    // `container.firstChild`, а у `document` его нет вовсе.
    if (String(location.search).indexOf('lookahead=2') >= 0) {
      Object.defineProperty(document, 'firstChild', {
        configurable: true, get: function () { return document.childNodes[0] || null; },
      });
      Object.defineProperty(document, 'lastChild', {
        configurable: true,
        get: function () { var c = document.childNodes; return c[c.length - 1] || null; },
      });
      rep('lookahead2: firstChild=' + (document.firstChild && document.firstChild.nodeName));
    }
    rep('nodeType of document = ' + document.nodeType);
    rep('documentElement.parentNode = ' + (document.documentElement.parentNode === document
      ? 'document' : String(document.documentElement.parentNode)));
    rep('document.documentElement = ' + (document.documentElement && document.documentElement.tagName));
    try {
      ReactDOM.hydrateRoot(document, e(Doc), {
        onRecoverableError: function (err) { rep('recoverable: ' + (err && err.message)); },
      });
      rep('hydrateRoot(document) returned');
    } catch (err) {
      rep('THROW hydrateRoot(document): ' + (err && err.stack ? err.stack : err));
    }

    setTimeout(function () {
      var keys = [];
      for (var k in document) { if (k.indexOf('__react') === 0) { keys.push(k); } }
      rep('document react keys: ' + (keys.length ? keys.join(',') : 'NONE'));
      var h1 = document.getElementsByTagName('h1')[0];
      var hkeys = [];
      if (h1) { for (var k2 in h1) { if (k2.indexOf('__react') === 0) { hkeys.push(k2); } } }
      rep('h1 react keys: ' + (hkeys.length ? hkeys.join(',') : 'NONE'));
      rep('h1 text: ' + (h1 ? h1.textContent : 'no h1'));
      rep('DONE');
    }, 800);
  }
})();
