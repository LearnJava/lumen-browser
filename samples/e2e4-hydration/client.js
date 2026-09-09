// E2E-4: клиентская половина пробы гидрации. Компоненты повторяют gen.js байт в байт,
// иначе React отчитается о рассинхроне разметки, а не о дефекте движка.
(function () {
  var rep = window.__rep;
  try { main(rep); } catch (err) { rep('THROW top: ' + (err && err.stack ? err.stack : err)); }

  function main(rep) {
  var e = React.createElement;

  function Slow() {
    return e('p', { id: 'slow' }, 'suspense payload');
  }

  function App() {
    var s = React.useState(0);
    var n = s[0], setN = s[1];
    return e(
      'div',
      { id: 'app' },
      e('h1', null, 'hydration probe'),
      e(React.Suspense, { fallback: e('p', null, 'loading') }, e(Slow)),
      e('button', { id: 'btn', onClick: function () { setN(n + 1); } }, 'clicked ' + n),
    );
  }

  rep('react=' + (typeof React) + ' version=' + React.version);
  rep('reactdom=' + (typeof ReactDOM) + ' hydrateRoot=' + (typeof ReactDOM.hydrateRoot));

  var container = document.getElementById('root');
  rep('container=' + (container ? 'ok' : 'MISSING'));

  try {
    var root = ReactDOM.hydrateRoot(container, e(App), {
      onRecoverableError: function (err) { rep('recoverable: ' + (err && err.message)); },
    });
    window.__root = root;
    rep('hydrateRoot returned');
  } catch (err) {
    rep('THROW hydrateRoot: ' + (err && err.stack ? err.stack : err));
  }

  // Признак того, что гидрация реально доехала: React вешает на контейнер
  // внутренний ключ, а на кнопку — свои props.
  setTimeout(function () {
    var btn = document.getElementById('btn');
    var keys = [];
    for (var k in container) { if (k.indexOf('__react') === 0) { keys.push(k); } }
    rep('container react keys: ' + (keys.length ? keys.join(',') : 'NONE'));
    var bkeys = [];
    if (btn) { for (var k2 in btn) { if (k2.indexOf('__react') === 0) { bkeys.push(k2); } } }
    rep('button react keys: ' + (bkeys.length ? bkeys.join(',') : 'NONE'));
    rep('button text after hydrate: ' + (btn ? btn.textContent : 'no button'));
    // Разделяем «клик не долетел до кнопки» и «клик не всплыл до корня React».
    // React 18 вешает свои слушатели на контейнер, а не на сам элемент.
    function tinfo(ev) {
      var t = ev && ev.target;
      return ' target=' + (t ? ((t.id || '') + '/' + (t.tagName || t.nodeName)) : String(t))
        + ' phase=' + (ev && ev.eventPhase);
    }
    if (btn) { btn.addEventListener('click', function (ev) { rep('native click on #btn' + tinfo(ev)); }); }
    container.addEventListener('click', function (ev) { rep('native click bubbled to #root' + tinfo(ev)); });
    document.addEventListener('click', function (ev) { rep('native click bubbled to document' + tinfo(ev)); });
    var app = document.getElementById('app');
    if (app) { app.addEventListener('click', function (ev) { rep('native click bubbled to #app' + tinfo(ev)); }); }
    var r = btn && btn.getBoundingClientRect && btn.getBoundingClientRect();
    if (r) { rep('btn rect x=' + r.x + ' y=' + r.y + ' w=' + r.width + ' h=' + r.height); }
    rep('DONE');
  }, 500);
  }
})();
